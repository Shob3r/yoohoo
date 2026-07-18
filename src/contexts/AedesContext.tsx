import { join } from "@tauri-apps/api/path";
import { fetch } from "@tauri-apps/plugin-http";
import { type ComponentChildren, createContext } from "preact";
import { useEffect, useRef, useState } from "preact/hooks";
import { convertFileSrc, exists, mkdir, readDir, remove } from "../lib/Fs";
import { getOption, setOption } from "../lib/Settings";
import { gameCodeToVariant } from "../lib/VariantConverter";
import { downloadFileNoProgress } from "../lib/Web";
import type { AedesAssetPaths, GameCodes, ResolvedAssetPaths } from "../types";

interface AedesContextType {
	cachedPaths: AedesAssetPaths | null;
	resolvedAssets: ResolvedAssetPaths | null;
	isLoading: boolean;
	error: string | null;
}

export const AedesContext = createContext<AedesContextType>({
	cachedPaths: null,
	resolvedAssets: null,
	isLoading: true,
	error: null,
});

const AEDES_ASSETS_BASE = "https://aedes.elysiae.app/";
const ASSET_DATA_URL = `${AEDES_ASSETS_BASE}/assets/assetData.json`;

const toCachePath = (path: string): string => {
	const parts = path.split("/");
	const [, assetType, gameCode, ...rest] = parts;
	return ["cache", gameCode, assetType, ...rest].join("/");
};

const resolveAssets = async (
	data: AedesAssetPaths,
): Promise<ResolvedAssetPaths> => {
	const resolved = {} as ResolvedAssetPaths;
	const entries = Object.entries(data) as [
		GameCodes,
		AedesAssetPaths[GameCodes],
	][];
	await Promise.all(
		entries.map(async ([gameCode, paths]) => {
			const variant = gameCodeToVariant[gameCode];
			const [icon, overlay, backgrounds] = await Promise.all([
				convertFileSrc(toCachePath(paths.icon)),
				convertFileSrc(toCachePath(paths.overlay)),
				Promise.all(
					paths.backgrounds.map(async (bg) => ({
						image: await convertFileSrc(toCachePath(bg.image)),
						video: bg.video
							? await convertFileSrc(toCachePath(bg.video))
							: null,
					})),
				),
			]);
			resolved[variant] = { icon, overlay, backgrounds };
		}),
	);
	return resolved;
};

const hasAnyAssets = (data: AedesAssetPaths): boolean =>
	Object.values(data).some(
		(paths) =>
			paths.icon !== "" ||
			paths.overlay !== "" ||
			paths.backgrounds.some(
				(bg) => bg.image !== "" || (bg.video ?? "") !== "",
			),
	);

const collectEndpointPaths = (data: AedesAssetPaths): string[] => {
	const paths: string[] = [];
	for (const p of Object.values(data)) {
		if (p.icon) paths.push(p.icon);
		if (p.overlay) paths.push(p.overlay);
		for (const bg of p.backgrounds) {
			if (bg.image) paths.push(bg.image);
			if (bg.video) paths.push(bg.video);
		}
	}
	return paths;
};

const collectLocalPaths = async (): Promise<string[]> => {
	const paths: string[] = [];
	await Promise.all(
		["bh3", "hk4e", "hkrpg", "nap"].flatMap((gameCode) =>
			["bg", "overlay", "icon"].map(async (assetType) => {
				const dir = await join("cache", gameCode, assetType);
				if (!(await exists(dir))) {
					await mkdir(dir);
					return;
				}
				const files = await readDir(dir);
				const filePaths = await Promise.all(
					files.map((file) => join(dir, file.name)),
				);
				paths.push(...filePaths);
			}),
		),
	);
	return paths;
};

export const AedesProvider = ({
	children,
}: {
	children: ComponentChildren;
}) => {
	const [cachedPaths, setCachedPaths] = useState<AedesAssetPaths | null>(null);
	const [resolvedAssets, setResolvedAssets] =
		useState<ResolvedAssetPaths | null>(null);
	const [isLoading, setIsLoading] = useState(true);
	const [error, setError] = useState<string | null>(null);
	const abortRef = useRef(false);

	useEffect(() => {
		abortRef.current = false;

		const init = async () => {
			let hasData = false;

			try {
				const saved = await getOption<AedesAssetPaths>("cachedBackgrounds");
				if (!abortRef.current && saved && hasAnyAssets(saved)) {
					setCachedPaths(saved);
					setResolvedAssets(await resolveAssets(saved));
					hasData = true;
				}
			} catch {
				// proceed to sync
			}

			if (abortRef.current) return;
			setIsLoading(false);

			try {
				const data: AedesAssetPaths = await fetch(ASSET_DATA_URL).then((r) =>
					r.json(),
				);
				if (abortRef.current) return;

				const endpointPaths = collectEndpointPaths(data);
				const expectedLocal = new Set(endpointPaths.map(toCachePath));
				const actualLocal = new Set(await collectLocalPaths());

				const toDownload = endpointPaths.filter(
					(path) => !actualLocal.has(toCachePath(path)),
				);
				const toDelete = [...actualLocal].filter(
					(path) => !expectedLocal.has(path),
				);

				if (toDownload.length > 0) {
					await Promise.all(
						toDownload.map(async (path) => {
							if (abortRef.current) return;
							await downloadFileNoProgress(
								`${AEDES_ASSETS_BASE}/${path}`,
								toCachePath(path),
							);
						}),
					);
				}

				if (toDelete.length > 0) {
					await Promise.all(
						toDelete.map(async (path) => {
							if (await exists(path)) {
								await remove(path);
							}
						}),
					);
				}

				const changed = toDownload.length > 0 || toDelete.length > 0;
				if (changed && !abortRef.current) {
					setCachedPaths(data);
					setResolvedAssets(await resolveAssets(data));
					await setOption("cachedBackgrounds", data);
					hasData = true;
				}
			} catch {
				// sync failure is fine if cached data exists
			}

			if (!hasData && !abortRef.current) {
				setError("Failed to load assets");
			}
		};

		init();
		return () => {
			abortRef.current = true;
		};
	}, []);

	return (
		<AedesContext.Provider
			value={{ cachedPaths, resolvedAssets, isLoading, error }}
		>
			{children}
		</AedesContext.Provider>
	);
};
