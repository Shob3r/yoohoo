import { useEffect, useState } from "preact/hooks";
import { useDownload } from "../../hooks/useDownload";
import { useGame } from "../../hooks/useGame";
import type { DownloadType } from "../../contexts/DownloadContext";
import {
	applyUpdate,
	checkGameUpdate,
	downloadGame,
	downloadUpdate,
	isGameInstalled,
	resumeDownloadInterrupted,
	runGame,
} from "../../lib/GameDownloader";
import {
	protonEnvAvailable,
	updateAllProtonComponents,
} from "../../lib/ProtonManager";
import {
	gameCodeToVariant,
	variantToGameCode,
} from "../../lib/VariantConverter";
import type { GameCodes } from "../../types";
import Button from "../Button";

const resolveButtonLabel = (
	protonAvailable: boolean,
	isSettingUpProton: boolean,
	canResume: boolean,
	gameInstalled: boolean,
	showUpdate: boolean,
	updateAvailable: boolean,
	downloadActive: boolean,
	isDownloadForActiveGame: boolean,
	downloadType: DownloadType,
	resumeDownloadType: DownloadType | "fresh" | undefined,
): string => {
	if (!protonAvailable) {
		return isSettingUpProton ? "Setting Up" : "Create Env";
	}
	if (canResume) {
		if (downloadActive && isDownloadForActiveGame) {
			return resumeDownloadType === "update" ? "Updating" : "Downloading";
		}
		if (resumeDownloadType === "update") return "Resume Update";
		if (resumeDownloadType === "preinstall") return "Resume Preinstall";
		return "Resume Download";
	}
	if (!gameInstalled) {
		return downloadActive && isDownloadForActiveGame
			? "Downloading"
			: "Download";
	}
	if (showUpdate) {
		return downloadActive && isDownloadForActiveGame ? "Updating" : "Update";
	}
	if (updateAvailable) {
		return downloadActive && isDownloadForActiveGame ? "Updating" : "Update";
	}
	if (downloadActive && isDownloadForActiveGame) {
		return downloadType === "update" ? "Updating" : "Downloading";
	}
	return "Play";
};

export const InstallerButton = () => {
	const { game } = useGame();
	const {
		state,
		setDownloadingGame,
		setDownloadType,
		setResumable,
		setProtonSetupProgress,
	} = useDownload();
	const [protonAvailable, setProtonAvailable] = useState<boolean>(false);
	const [gameInstalled, setGameInstalled] = useState<boolean>(false);
	const [updateAvailable, setUpdateAvailable] = useState<boolean>(false);
	const [preinstallDownloaded, setPreinstallDownloaded] =
		useState<boolean>(false);

	const downloadActive =
		state.isDownloading ||
		state.isAssembling ||
		state.isVerifying ||
		state.isFetchingManifest ||
		state.isPaused ||
		state.isApplyingPreinstall;
	const isDownloadForActiveGame = state.downloadingGame === game;
	const canResume =
		state.isResumable &&
		state.resumeInfo !== null &&
		variantToGameCode[game] === state.resumeInfo.gameId &&
		(state.resumeInfo.downloadType !== "fresh" || !gameInstalled);

	useEffect(() => {
		let cancelled = false;
		protonEnvAvailable().then((res) => {
			if (!cancelled) setProtonAvailable(res);
		});
		isGameInstalled(game).then((res) => {
			if (!cancelled) setGameInstalled(res);
		});
		return () => {
			cancelled = true;
		};
	}, [game]);

	useEffect(() => {
		if (!gameInstalled) return;
		let cancelled = false;
		checkGameUpdate(game).then((res) => {
			if (!cancelled && res) {
				setUpdateAvailable(res.updateAvailable);
				setPreinstallDownloaded(res.preinstallDownloaded);
			}
		});
		return () => {
			cancelled = true;
		};
	}, [game, gameInstalled, state.isFinished]);

	useEffect(() => {
		if (state.isFinished && isDownloadForActiveGame) {
			setGameInstalled(true);
		}
	}, [state.isFinished, isDownloadForActiveGame]);

	const showUpdate = updateAvailable && gameInstalled;

	const resumeVariant = state.resumeInfo
		? gameCodeToVariant[state.resumeInfo.gameId as GameCodes]
		: null;

	return (
		<div class="flex w-auto flex-row gap-x-3.5">
			<Button
				variant="primary"
				width={13.75}
				height={4.06}
				disabled={downloadActive || state.isSettingUpProton}
				onClick={async () => {
					if (!protonAvailable) {
						await updateAllProtonComponents((event) => {
							setProtonSetupProgress(event);
						});
						setProtonAvailable(true);
					} else if (canResume && resumeVariant !== null) {
						setResumable(null);
						if (state.resumeInfo?.downloadType === "update") {
							setDownloadType("update");
						} else if (state.resumeInfo?.downloadType === "preinstall") {
							setDownloadType("preinstall");
						} else {
							setDownloadType("install");
						}
						setDownloadingGame(resumeVariant);
						await resumeDownloadInterrupted();
					} else if (!gameInstalled) {
						setDownloadType("install");
						setDownloadingGame(game);
						await downloadGame(game);
					} else if (showUpdate) {
						setDownloadType("update");
						setDownloadingGame(game);
						if (preinstallDownloaded) {
							await applyUpdate(game);
						} else {
							await downloadUpdate(game, false);
						}
					} else {
						if (updateAvailable) {
							setDownloadType("update");
							setDownloadingGame(game);
							if (preinstallDownloaded) {
								await applyUpdate(game);
							} else {
								await downloadUpdate(game, false);
							}
						} else {
							await runGame(game);
						}
					}
				}}
			>
				{resolveButtonLabel(
					protonAvailable,
					state.isSettingUpProton,
					canResume,
					gameInstalled,
					showUpdate,
					updateAvailable,
					downloadActive,
					isDownloadForActiveGame,
					state.downloadType,
					state.resumeInfo?.downloadType,
				)}
			</Button>
		</div>
	);
};

export default InstallerButton;
