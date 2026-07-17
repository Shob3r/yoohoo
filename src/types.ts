/** biome-ignore-all lint/suspicious/noExplicitAny: Types that use any need them. Currently, those types fetch from an online webpoint that currently returns null but could be updated in the future */
import type React from "react";

// Enums

export enum Variants {
	BH3,
	HK4E,
	HKRPG,
	NAP,
}

export enum BhServers {
	GLB,
	JP,
	KR,
	SEA,
	TW,
}

// Primitive / Alias Types

export type GameCodes = "bh3" | "hk4e" | "hkrpg" | "nap";
export type AppModules = "proton" | "jadeite";
export type ComponentSize = "xs" | "sm" | "md" | "lg" | "xl";

// Game

export type GameData = {
	gameCode: string;
	gameDir: string;
	requestedLanguage: string;
};

export type ResumeInfo = {
	gameId: string;
	downloadType: "fresh" | "update" | "preinstall";
};

// Proton / Components

export type ProtonComponent = {
	componentName: AppModules;
	extractTo: string;
	saveTo: string;
	postInstall?: () => Promise<void>;
};

export type ProtonComponentData = {
	proton: string | null;
	jadeite: string | null;
};

export type ModuleData = {
	tag: string;
	download_url: string;
	hash: string;
};

// Sophon / Download Progress

export type SophonProgress =
	| { type: "fetchingManifest" }
	| { type: "calculatingDownloads"; checkedFiles: number; totalFiles: number }
	| {
			type: "downloading";
			downloadedBytes: number;
			totalBytes: number;
			speedBps: number;
			etaSeconds: number;
	  }
	| { type: "paused"; downloadedBytes: number; totalBytes: number }
	| { type: "assembling"; assembledFiles: number; totalFiles: number }
	| { type: "checkingFiles"; checkedFiles: number; totalFiles: number }
	| {
			type: "verifying";
			scannedFiles: number;
			totalFiles: number;
			errorCount: number;
	  }
	| { type: "warning"; message: string }
	| { type: "error"; message: string }
	| { type: "installingPlugins"; currentPlugin: string; totalPlugins: number }
	| {
			type: "downloadingPlugin";
			name: string;
			downloadedBytes: number;
			totalBytes: number;
	  }
	| { type: "applyingPreinstall"; appliedFiles: number; totalFiles: number }
	| { type: "finished" };

export type ProtonSetupProgress =
	| {
			type: "protonSetupDownloading";
			component: string;
			downloaded_bytes: number;
			total_bytes: number;
	  }
	| { type: "protonSetupExtracting"; component: string }
	| { type: "protonSetupInstalling"; component: string }
	| { type: "protonSetupFinished" };

// Modal

export type ModalHandle = {
	open: () => void;
	close: () => void;
	toggle: (state: boolean) => void;
};

export type ModalProps = {
	title?: string;
	width?: number;
	height?: number;
	closeable?: boolean;
	children: React.ReactNode;
};

// Settings / Options

export type Settings = {
	version: number;
	isFirstLaunch: boolean;
	lastUsedVersion: string;
	selectedGame: GameCodes;
	voLanguage: string;
	blockNotifications: boolean;
	createShortcuts: boolean;
	autoUpdate: boolean;
	autoPreload: boolean;
	installedComponents: InstalledComponentsData;
	// Elysiae does not just cache backgrounds now, but the naming scheme is kept to maintain compatibility with older versions of the app
	cachedBackgrounds: AedesAssetPaths;
};

export type InstalledComponentsData = {
	proton: string | null;
	jadeite: string | null;
};

type BaseOption<T, V> = {
	name: string;
	type: T;
	getValue: () => Promise<V>;
	setValue: (value: V) => Promise<void>;
};

type DropdownOption = BaseOption<"dropdown", string> & {
	labels: string[];
	values: string[];
};

type BooleanOption = BaseOption<"boolean", boolean>;

export type Option = DropdownOption | BooleanOption;

// This looks really silly. The cached Background data type is how the settings file will store cached background data from now on. It is identical to what each "game category" of the endpoint looks like

export type AedesAssetPaths = {
	[key in GameCodes]: CachedBackgroundPaths;
};

// The resolved cache is keyed by `Variants` because `resolveAssets` remaps each
// GameCodes-string key to its numeric enum value via `gameCodeToVariant`.
export type ResolvedAssetPaths = {
	[key in Variants]: CachedBackgroundPaths;
};

export type CachedBackgroundPaths = {
	backgrounds: {
		image: string;
		video: string | null;
	}[];
	icon: string;
	overlay: string;
};
