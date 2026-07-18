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
const GAME_CODES = ["bh3", "hk4e", "hkrpg", "nap"] as const;

const normalize = (raw: unknown): AedesAssetPaths | null => {
	if (!raw || typeof raw !== "object") return null;
	const data = {} as AedesAssetPaths;
	for (const code of GAME_CODES) {
		const src = (raw as Record<string, unknown>)[code] as
			| Record<string, unknown>
			| undefined;
		const bgs = Array.isArray(src?.backgrounds) ? src.backgrounds : [];
		data[code] = {
			icon: typeof src?.icon === "string" ? src.icon : "",
			overlay: typeof src?.overlay === "string" ? src.overlay : "",
			backgrounds: bgs.map((bg: unknown) => {
				const b = bg as Record<string, unknown> | undefined;
				return {
					image: typeof b?.image === "string" ? b.image : "",
					video: typeof b?.video === "string" ? b.video : null,
				};
			}),
		};
	}
	return data;
};

const toCachePath = (path: string): string => {
	if (!path) return "";
	const parts = path.split("/");
	const [, assetType, gameCode, ...rest] = parts;
	return ["cache", gameCode, assetType, ...rest].join("/");
};

const resolvePath = async (path: string | null): Promise<string> => {
	const cached = toCachePath(path ?? "");
	return cached ? convertFileSrc(cached) : "";
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
				resolvePath(paths.icon),
				resolvePath(paths.overlay),
				Promise.all(
					paths.backgrounds.map(async (bg) => ({
						image: await resolvePath(bg.image),
						video: await resolvePath(bg.video),
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
		(p) =>
			p.icon !== "" ||
			p.overlay !== "" ||
			p.backgrounds.some((bg) => bg.image !== "" || bg.video !== null),
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
		GAME_CODES.flatMap((gameCode) =>
			(["bg", "overlay", "icon"] as const).map(async (assetType) => {
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

			let saved: AedesAssetPaths | null = null;
			try {
				const raw = await getOption("cachedBackgrounds");
				saved = normalize(raw);
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
				const raw = await fetch(ASSET_DATA_URL).then((r) => r.json());
				const data = normalize(raw);
				if (!data || !hasAnyAssets(data) || abortRef.current) return;

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

				const changed = toDownload.length > 0 || toDelete.length > 0 || JSON.stringify(data) !== JSON.stringify(saved);
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
