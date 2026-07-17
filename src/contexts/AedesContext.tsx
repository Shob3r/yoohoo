import { join } from "@tauri-apps/api/path";
import { fetch } from "@tauri-apps/plugin-http";
import { type ComponentChildren, createContext } from "preact";
import { useEffect, useRef, useState } from "preact/hooks";
import { convertFileSrc, exists, mkdir, readDir, remove } from "../lib/Fs";
import { getOption, setOption } from "../lib/Settings";
import { gameCodeToVariant } from "../lib/VariantConverter";
import { downloadFileNoProgress } from "../lib/Web";
import type { AedesAssetPaths, GameCodes, Variants } from "../types";

interface AedesContextType {
	cachedPaths: AedesAssetPaths | null;
	resolvedAssets: AedesAssetPaths | null;
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

const toCachePath = (path: string): string => {
	const parts = path.split("/");
	const [, assetType, gameCode, ...rest] = parts;
	return ["cache", gameCode, assetType, ...rest].join("/");
};

const resolveAssets = async (
	data: AedesAssetPaths,
): Promise<AedesAssetPaths> => {
	const resolved = {} as AedesAssetPaths;
	const entries = Object.entries(data) as [
		GameCodes,
		AedesAssetPaths[Variants],
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

const basenameOf = (path: string): string => {
	return path.split("/").pop() ?? path;
};

export const AedesProvider = ({
	children,
}: {
	children: ComponentChildren;
}) => {
	const [fsCache, setCache] = useState<AedesAssetPaths | null>(null);
	const [resolvedCache, setResolvedCache] = useState<AedesAssetPaths | null>(
		null,
	);
	const [isLoading, setIsLoading] = useState<boolean>(true);
	const [error, setError] = useState<string | null>(null);
	const fetchCountRef = useRef(0);

	/*
	 * Implementation method:
	 * On Launch:
	 *  1. fetch cached path data from settings file, set cache variable to that
	 *
	 * Cache updater:
	 *  1. Find files that exist locally but are on the endpoint
	 *  2. Find files that exist on the endpoint but don't exist locally
	 *  3. Download all files found in step 3
	 *  4. Update cached data path setting, refresh cache variable
	 *  5. Delete all files found in step 2
	 *  backgrounds images/videos, game overlays, and game icons are all stored in their own folder in appDataDir().
	 */

	const fetchData = async () => {
		const fetchId = ++fetchCountRef.current;

		setIsLoading(true);
		setError(null);

		try {
			const saved = await getOption<AedesAssetPaths>("cachedBackgrounds");
			if (saved && fetchId === fetchCountRef.current) {
				setCache(saved);
				setResolvedCache(await resolveAssets(saved));
			}

			const data: AedesAssetPaths = await fetch(
				`${AEDES_ASSETS_BASE}/assets/assetData.json`,
			).then((r) => r.json());

			const endpointPaths: string[] = [];
			const localPaths: string[] = [];

			// Get Web paths
			for (const [_, paths] of Object.entries(data)) {
				endpointPaths.push(paths.icon);
				endpointPaths.push(paths.overlay);
				paths.backgrounds.forEach((bg) => {
					endpointPaths.push(bg.image);
					if (bg.video) endpointPaths.push(bg.video);
				});
			}

			// Get local paths:
			// Iterate through each directory and subdirectory in the appdata cache folder to find file paths that exist on disk
			await Promise.all(
				["bh3", "hk4e", "hkrpg", "nap"].flatMap((gameCode) =>
					["bg", "overlay", "icon"].map(async (assetType) => {
						const dir = await join("cache", gameCode, assetType);
						if (!(await exists(dir))) {
							await mkdir(dir);
							return;
						}
						const files = await readDir(dir);
						const paths = await Promise.all(
							files.map((file) => join(dir, file.name)),
						);
						localPaths.push(...paths);
					}),
				),
			);

			// Get download/remove arrays (compared by basename so prefix
			// and gameCode/assetType ordering differences don't break the diff)
			const localBasenames = new Set(localPaths.map(basenameOf));
			const endpointBasenames = new Set(endpointPaths.map(basenameOf));
			const toDownload: string[] = endpointPaths.filter(
				(path) => !localBasenames.has(basenameOf(path)),
			);
			const toDelete: string[] = localPaths.filter(
				(path) => !endpointBasenames.has(basenameOf(path)),
			);

			// Download new files
			await Promise.all(
				toDownload.map(async (path) => {
					const url = `${AEDES_ASSETS_BASE}/${path}`;
					await downloadFileNoProgress(url, toCachePath(path));
				}),
			);

			// Update cache tracker
			if (toDownload.length > 0 && fetchId === fetchCountRef.current) {
				setCache(data);
				setResolvedCache(await resolveAssets(data));
				await setOption<AedesAssetPaths>("cachedBackgrounds", data);
			}

			// Delete old local files
			await Promise.all(
				toDelete.map(async (path) => {
					if (await exists(path)) {
						await remove(path);
					}
				}),
			);
		} catch (err) {
			setError(err instanceof Error ? err.message : String(err));
			console.error(err);
		} finally {
			setIsLoading(false);
		}
	};

	useEffect(() => {
		fetchData();
	}, []); // Runs each time the game state switches to fetch new background assets if they are updated while Elysiae is being used

	return (
		<AedesContext.Provider
			value={{
				cachedPaths: fsCache,
				resolvedAssets: resolvedCache,
				isLoading: isLoading,
				error: error,
			}}
		>
			{children}
		</AedesContext.Provider>
	);
};
