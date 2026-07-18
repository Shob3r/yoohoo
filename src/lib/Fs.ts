import {
	invoke,
	convertFileSrc as tauriConvertFileSrc,
} from "@tauri-apps/api/core";
import { appDataDir, join } from "@tauri-apps/api/path";

import {
	BaseDirectory as BaseDir,
	type DirEntry,
	exists as tauriExists,
	mkdir as tauriMkdir,
	readDir as tauriReadDir,
	readFile as tauriReadFile,
	readTextFile as tauriReadTextFile,
	remove as tauriRemove,
	rename as tauriRename,
	writeFile as tauriWriteFile,
	writeTextFile as tauriWriteTextFile,
} from "@tauri-apps/plugin-fs";
import { error, info } from "@tauri-apps/plugin-log";

/**
 * Tauri's exists() function, with the base directory set to the app data directory
 */
export const exists = async (path: string): Promise<boolean> => {
	return new Promise((resolve, reject) => {
		tauriExists(path, { baseDir: BaseDir.AppData }).then(resolve).catch(reject);
	});
};

/**
 * Tauri's readFile() function, with the base directory set to the app data directory
 */
export const readFile = async (
	path: string,
): Promise<Uint8Array<ArrayBuffer>> => {
	return new Promise((resolve, reject) => {
		tauriReadFile(path, { baseDir: BaseDir.AppData })
			.then(resolve)
			.catch(reject);
	});
};

/**
 * Tauri's readTextFile() function, with the base directory set to the app data directory
 */
export const readTextFile = async (path: string): Promise<string> => {
	return new Promise((resolve, reject) => {
		tauriReadTextFile(path, { baseDir: BaseDir.AppData })
			.then(resolve)
			.catch(reject);
	});
};

/**
 * Tauri's writeFile() function, with the base directory set to the app data directory
 */
export const writeFile = async (
	path: string,
	contents:
		| ReadableStream<Uint8Array<ArrayBufferLike>>
		| Uint8Array<ArrayBufferLike>,
): Promise<void> => {
	return new Promise((resolve, reject) => {
		tauriWriteFile(path, contents, {
			baseDir: BaseDir.AppData,
		})
			.then(resolve)
			.catch(reject);
	});
};

/**
 * Tauri's writeTextFile() function, with the base directory set to the app data directory
 */
export const writeTextFile = async (
	path: string,
	contents: string,
): Promise<void> => {
	return new Promise((resolve, reject) => {
		tauriWriteTextFile(path, contents, {
			baseDir: BaseDir.AppData,
		})
			.then(resolve)
			.catch(reject);
	});
};

/**
 * Tauri's remove() function configured for removing a single file, with the base directory set to the app data directory
 */
export const remove = async (path: string): Promise<void> => {
	return new Promise((resolve, reject) => {
		tauriRemove(path, { baseDir: BaseDir.AppData }).then(resolve).catch(reject);
	});
};

/**
 * Tauri's remove() function configured for removing a directory, with the base directory set to the app data directory
 */
export const removeDir = async (path: string): Promise<void> => {
	return new Promise((resolve, reject) => {
		tauriRemove(path, { recursive: true, baseDir: BaseDir.AppData })
			.then(resolve)
			.catch(reject);
	});
};

/**
 * Tauri's mkdir() function, with the base directory set to the app data directory
 */
export const mkdir = async (path: string): Promise<void> => {
	return new Promise((resolve, reject) => {
		tauriMkdir(path, { baseDir: BaseDir.AppData, recursive: true })
			.then(resolve)
			.catch(reject);
	});
};

/**
 * Tauri's rename() function, with the base directory set to the app data directory
 */
export const rename = async (
	originalPath: string,
	destPath: string,
): Promise<void> => {
	return new Promise((resolve, reject) => {
		tauriRename(originalPath, destPath, {
			newPathBaseDir: BaseDir.AppData,
			oldPathBaseDir: BaseDir.AppData,
		})
			.then(resolve)
			.catch(reject);
	});
};

/**
 * Tauri's readDir() function, with the base directory set to the app data directory
 */
export const readDir = async (path: string): Promise<DirEntry[]> => {
	return new Promise((resolve, reject) => {
		tauriReadDir(path, { baseDir: BaseDir.AppData })
			.then((res) => {
				resolve(res);
			})
			.catch(reject);
	});
};

/**
 * Gets all file names in a path, relative to the app data directory
 * @param path A valid path to a directory
 * @returns List all files found in the `path` parameter
 */
export const getDirFileNames = async (path: string): Promise<string[]> => {
	return new Promise((resolve, reject) => {
		readDir(path)
			.then((dirItems) => {
				const final: string[] = [];
				dirItems.map((i) => final.push(i.name));
				resolve(final);
			})
			.catch(reject);
	});
};

/**
 * Tauri's convertFileSrc() function, with the base directory set to the app data directory
 */
export const convertFileSrc = async (relativePath: string) => {
	const absolutePath = await join(await appDataDir(), relativePath);
	return tauriConvertFileSrc(absolutePath);
};

/**
 * Extracts a compressed archive to a specified location. Supports most common
 * tar compression formats (gz, xz, zstd) and zip
 *
 * @param archivePath Path to archive
 * @param dest Destination to extract to
 */
export const extractFile = async (
	archivePath: string,
	dest: string,
): Promise<void> => {
	info(archivePath);
	if (await exists(archivePath)) {
		await invoke("extract_file", { archive: archivePath, dest: dest });
		remove(archivePath);
	} else {
		error(`extractFile: "${archivePath}" does not exist`);
	}
};
