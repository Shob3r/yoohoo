import { appDataDir, join } from "@tauri-apps/api/path";
import { load, type Store } from "@tauri-apps/plugin-store";
import { type Settings, Variants } from "../types";
import { readTextFile, writeTextFile } from "./Fs";
import { invoke } from "@tauri-apps/api/core";

let store: Store | undefined;
const SETTINGS_PATH = "settings.json";
const CURRENT_DATA_VERSION = 1;

const loadStore = async (): Promise<Store> => {
	await updateSettingsData();

	// load() doesn't have any settings for a relative app, so an absolute path must be used instead
	return await load(await join(await appDataDir(), SETTINGS_PATH));
};

const updateSettingsData = async () => {
	let settingsData: Settings;
	try {
		settingsData = JSON.parse(await readTextFile(SETTINGS_PATH));
	} catch {
		settingsData = {} as Settings;
	}

	const updatedSettingsData: Settings = {
		version: settingsData.version ?? CURRENT_DATA_VERSION,
		isFirstLaunch: settingsData.isFirstLaunch ?? true,
		lastUsedVersion: settingsData.lastUsedVersion ?? await invoke<string>("elysiae_version"),
		selectedGame: settingsData.selectedGame ?? "hk4e",
		voLanguage: settingsData.voLanguage ?? "en",
		blockNotifications: settingsData.blockNotifications ?? false,
		createShortcuts: settingsData.createShortcuts ?? true,
		autoUpdate: settingsData.autoUpdate ?? true,
		autoPreload: settingsData.autoPreload ?? true,
		installedComponents: settingsData.installedComponents ?? {
			proton: null,
			jadeite: null,
		},
		cachedBackgrounds: settingsData.cachedBackgrounds ?? {
			[Variants.BH3]: [],
			[Variants.HK4E]: [],
			[Variants.HKRPG]: [],
			[Variants.NAP]: [],
		},
	};

	await writeTextFile(SETTINGS_PATH, JSON.stringify(updatedSettingsData));
	await migrateSettings(updatedSettingsData);
};

const migrateSettings = async (data: Settings) => {
	for (let i = data.version; i < CURRENT_DATA_VERSION; i++) {
		switch (
			data.version
			// unused as no migration is needed right now
		) {
		}
	}
};

export const getOption = async <T = unknown>(
	key: keyof Settings,
): Promise<T | undefined> => {
	if (!store) {
		store = await loadStore();
	}
	return store?.get<T>(key);
};

export const setOption = async <T = unknown>(
	key: keyof Settings,
	value: T,
): Promise<void> => {
	if (!store) {
		store = await loadStore();
	}
	await store?.set(key, value);
	await store.save();
	await store.reload();
};
