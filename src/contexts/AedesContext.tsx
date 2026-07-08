import { type ComponentChildren, createContext } from "preact";
import { useEffect, useState } from "preact/hooks";
import type { AedesAssetPaths } from "../types";
import { useGame } from "../hooks/useGame";
import { fetch } from "@tauri-apps/plugin-http";

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
	 * Implementation strategy:
	 * 
	 * 
	 * Cache updater: 
	 * 	1. fetch cached path data from settings file, set cache variable to that
	 *  2. Find files that exist locally but are on the endpoint
	 *  3. Find files that exist on the endpoint but don't exist locally
	 *  4. Delete all files found in step 2
	 *  5. Download all files found in step 3
	 *  6. Update cached data path setting, refresh cache variable
	 * 
	 *  backgrounds images/videos, game overlays, and game icons are all stored in their own folder in appDataDir().
	 */

	useEffect(() => {
		fetch("https://aedes.elysiae.app/assets/assetData.json").then((data) => {
			data.json().then((json: AedesAssetPaths) => {
				for(const key in json) {

				}
			})
		})
	}, [game]);

	return null;
};
