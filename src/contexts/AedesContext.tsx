import { join } from "@tauri-apps/api/path";
import { fetch } from "@tauri-apps/plugin-http";
import { type ComponentChildren, createContext } from "preact";
import { useEffect, useState } from "preact/hooks";
import { useGame } from "../hooks/useGame";
import { exists, mkdir, readDir, remove } from "../lib/Fs";
import { getOption, setOption } from "../lib/Settings";
import { downloadFileNoProgress } from "../lib/Web";
import type { AedesAssetPaths } from "../types";

interface AedesContextType {
	cachedData: AedesAssetPaths | null;
	isLoading: boolean;
	error: string | null;
}

export const AedesContext = createContext<AedesContextType>({
	cachedData: null,
	isLoading: true,
	error: null,
});

const AEDES_ASSETS_BASE = "https://aedes.elysiae.app/";

/**
 * @author @Shob3r
 * @param source
 * @param compareTo
 * @returns elements in `compareTo` that are not present in `source`
 */
const getMissingItems = <T = unknown>(source: T[], compareTo: T[]): T[] => {
	const compareToSet = new Set(compareTo);
	return source.filter((item) => !compareToSet.has(item));
};

export const AedesProvider = ({
	children,
}: {
	children: ComponentChildren;
}) => {
	const [cache, setCache] = useState<AedesAssetPaths | null>(null);
	const [isLoading, setIsLoading] = useState<boolean>(true);
	const [error, setError] = useState<string | null>(null);

	const { game } = useGame();

	/**
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

	useEffect(() => {
		(async () => {
			setIsLoading(true);
			try {
				const data: AedesAssetPaths = await (
					await fetch(`${AEDES_ASSETS_BASE}/assets/assetData.json`)
				).json();

				const endpointPaths: string[] = [];
				const localPaths: string[] = [];

				// Get Web paths
				for (const [_, paths] of Object.entries(data)) {
					endpointPaths.push(paths.icon);
					endpointPaths.push(paths.overlay);
					paths.backgrounds.forEach((bg) => {
						const toPush = [bg.image];
						if (bg.video) {
							toPush.push(bg.video);
						}
						endpointPaths.push(...toPush);
					});
				}

				// Get local paths:
				// Iterate through each directory and subdirectory in the appdata cache folder to find file paths that exist on disk
				await Promise.all(
					["bg", "overlay", "icon"].map(
						async (gameCode) =>
							await Promise.all(
								["bh3", "hk4e", "hkrpg", "nap"].map(async (assetType) => {
									const dir = await join("cache", gameCode, assetType);
									if (await exists(dir)) {
										const files = await readDir(dir);
										for (const file of files) {
											const path = await join(dir, file.name);
											localPaths.push(path);
										}
									} else {
										await mkdir(dir);
									}
								}),
							),
					),
				);

				// Get download/remove arrays
				const toDownload: string[] = getMissingItems<string>(endpointPaths, localPaths);
				const toDelete: string[] = getMissingItems<string>(localPaths, endpointPaths);

				// Download new files
				await Promise.all(
					toDownload.map(async (path) => {
						const url = `${AEDES_ASSETS_BASE}/${path}`;

						// Cache is a much more identifiable name for an asset cache directory, but assets makes more sense when storing assets on en endpoint
						const cachePath = path.replaceAll("assets", "cache"); 
						await downloadFileNoProgress(url, cachePath);
					}),
				);

				// Update cache tracker
				if (JSON.stringify(cache) !== JSON.stringify(data)) {
					setCache(data);
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
				console.error(error);
			} finally {
				setIsLoading(false);
			}
		})();
	}, [game]);

	// Initial cache setting
	useEffect(() => {
		getOption<AedesAssetPaths>("cachedBackgrounds").then((res) => {
			setCache(res as AedesAssetPaths);
		});
	}, []);

	return (
		<AedesContext.Provider
			value={{
				cachedData: cache,
				isLoading: isLoading,
				error: error,
			}}
		>
			{children}
		</AedesContext.Provider>
	);
};
