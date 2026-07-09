import { Pause, Play } from "lucide-preact";
import { useMemo } from "preact/hooks";
import { useDownload } from "../../hooks/useDownload";
import { useGame } from "../../hooks/useGame";
import { pauseDownload, resumeDownload } from "../../lib/GameDownloader";
import { variantToGameName } from "../../lib/VariantConverter";
import type { DownloadType } from "../../contexts/DownloadContext";
import Button from "../Button";
import Progressbar from "../Progressbar";

const formatNumber = (num: number): string => {
	try {
		return new Intl.NumberFormat(navigator.language).format(num);
	} catch {
		return new Intl.NumberFormat("en-US").format(num);
	}
};

type TitleContext = {
	isApplyingPreinstall: boolean;
	isPaused: boolean;
	isSettingUpProton: boolean;
	downloadInProgress: boolean;
	verifyingInProgress: boolean;
	assemblingInProgress: boolean;
	downloadType: DownloadType;
	gameName: string;
};

const resolveTitleText = (ctx: TitleContext): string => {
	if (ctx.isApplyingPreinstall) return "Applying Preinstall...";
	if (ctx.isPaused) {
		return ctx.downloadType === "update" ? "Update Paused" : "Download Paused";
	}
	if (ctx.isSettingUpProton) return "Setting Up Environment...";
	if (ctx.downloadInProgress) {
		if (ctx.downloadType === "update") {
			return `Downloading update for ${ctx.gameName}...`;
		}
		if (ctx.downloadType === "preinstall") {
			return `Downloading preinstall for ${ctx.gameName}...`;
		}
		return `Downloading ${ctx.gameName}...`;
	}
	if (ctx.verifyingInProgress) {
		return `Verifying files for ${ctx.gameName}...`;
	}
	if (ctx.assemblingInProgress) {
		return `Assembling chunks ${ctx.gameName}...`;
	}
	if (ctx.gameName !== "") {
		if (ctx.downloadType === "update") {
			return `Downloading update for ${ctx.gameName}...`;
		}
		if (ctx.downloadType === "preinstall") {
			return `Downloading preinstall for ${ctx.gameName}...`;
		}
		return `Downloading ${ctx.gameName}...`;
	}
	return "Downloading...";
};

export const DownloadProgress = () => {
	const { state } = useDownload();
	const { game } = useGame();
	const {
		isPaused,
		isDownloading,
		isAssembling,
		isCheckingFiles,
		isVerifying,
		isFetchingManifest,
		isCalculatingDownloads,
		isApplyingPreinstall,
		isError,
		isFinished,
		isSettingUpProton,
	} = state;

	const isActive =
		isDownloading ||
		isAssembling ||
		isCheckingFiles ||
		isVerifying ||
		isFetchingManifest ||
		isCalculatingDownloads ||
		isPaused ||
		isSettingUpProton ||
		isApplyingPreinstall ||
		state.downloadingGame !== null;
	if (!isActive && !isError && !isFinished) return null;
	if (isFinished) return null;

	const protonSetupPct = useMemo(() => {
		if (!isSettingUpProton || state.protonSetupDownloadTotal <= 0) return 0;
		if (state.protonSetupPhase !== "downloading") return 100;
		return (
			(state.protonSetupDownloadedBytes / state.protonSetupDownloadTotal) * 100
		);
	}, [
		isSettingUpProton,
		state.protonSetupPhase,
		state.protonSetupDownloadedBytes,
		state.protonSetupDownloadTotal,
	]);

	const protonPhaseLabel = useMemo(() => {
		switch (state.protonSetupPhase) {
			case "downloading":
				return "Downloading";
			case "extracting":
				return "Extracting";
			case "installing":
				return "Installing";
			default:
				return "";
		}
	}, [state.protonSetupPhase]);

	const protonDownloadedMB = (
		state.protonSetupDownloadedBytes /
		1024 ** 2
	).toFixed(1);
	const protonTotalMB = (state.protonSetupDownloadTotal / 1024 ** 2).toFixed(1);

	const derived = useMemo(() => {
		const downloadPct =
			state.downloadTotal > 0
				? (state.downloadedBytes / state.downloadTotal) * 100
				: 0;
		const assemblePct =
			state.totalFiles > 0
				? (state.assembledFiles / state.totalFiles) * 100
				: 0;
		const speedMB = state.speedBps / 1024 ** 2;
		const eta = state.etaSeconds;
		const etaStr =
			eta > 0
				? `${String(Math.floor(eta / 3600)).padStart(2, "0")}:${String(Math.floor((eta % 3600) / 60)).padStart(2, "0")}:${String(Math.floor(eta % 60)).padStart(2, "0")}`
				: "";
		const downloadedGB = (state.downloadedBytes / 1024 ** 3).toFixed(2);
		const totalGB = (state.downloadTotal / 1024 ** 3).toFixed(2);
		const verifyPct =
			state.totalFiles > 0 ? (state.scannedFiles / state.totalFiles) * 100 : 0;
		const calcPct =
			state.totalFiles > 0 ? (state.checkedFiles / state.totalFiles) * 100 : 0;
		const applyPct =
			state.totalFiles > 0
				? (state.assembledFiles / state.totalFiles) * 100
				: 0;
		const checkingPct =
			state.totalFiles > 0 ? (state.checkedFiles / state.totalFiles) * 100 : 0;
		return {
			downloadPct,
			assemblePct,
			checkingPct,
			speedMB,
			etaStr,
			downloadedGB,
			totalGB,
			verifyPct,
			calcPct,
			applyPct,
		};
	}, [
		state.downloadedBytes,
		state.downloadTotal,
		state.assembledFiles,
		state.totalFiles,
		state.speedBps,
		state.etaSeconds,
		state.scannedFiles,
		state.checkedFiles,
	]);

	const downloadComplete =
		state.downloadTotal > 0 && state.downloadedBytes >= state.downloadTotal;
	const verificationComplete =
		state.totalFiles > 0 && state.checkedFiles >= state.totalFiles;
	const assemblyComplete =
		state.totalFiles > 0 && state.assembledFiles >= state.totalFiles;

	const downloadInProgress = isDownloading && !downloadComplete;
	const verifyingInProgress =
		(isCheckingFiles && !verificationComplete) ||
		(isVerifying && state.scannedFiles < state.totalFiles);
	const assemblingInProgress = isAssembling && !assemblyComplete;

	const gameName = state.downloadingGame
		? variantToGameName[state.downloadingGame]
		: "";

	const titleText = resolveTitleText({
		isApplyingPreinstall,
		isPaused,
		isSettingUpProton,
		downloadInProgress,
		verifyingInProgress,
		assemblingInProgress,
		downloadType: state.downloadType,
		gameName,
	});

	const canPause = (isDownloading || isPaused) && !isVerifying;

	const showDownloadBar =
		isDownloading ||
		isPaused ||
		isVerifying ||
		isCheckingFiles ||
		(isAssembling && !isDownloading && !isPaused);
	const downloadBarFinished =
		!isDownloading &&
		!isPaused &&
		(isVerifying || isCheckingFiles || isAssembling);
	const downloadBarPct = downloadBarFinished ? 100 : derived.downloadPct;

	return (
		<div class="mr-10 flex h-auto w-[65%] flex-col items-start justify-start gap-y-3 rounded-lg bg-black/50 px-4 py-5 align-bottom">
			<div class="flex w-full flex-row items-center justify-between">
				<h1 class="-mt-2 mb-0.5 text-white">{titleText}</h1>
				{canPause && (
					<Button
						onClick={async () => {
							if (isPaused) {
								await resumeDownload();
							} else {
								await pauseDownload();
							}
						}}
						variant="secondary"
						width={1}
						height={1}
					>
						{isPaused ? (
							<Play className={"-m-1 leading-0"} />
						) : (
							<Pause className={"-m-1 leading-0"} />
						)}
					</Button>
				)}
			</div>
			{isSettingUpProton && (
				<div class="flex min-w-full flex-col gap-y-1 text-left">
					<h2 class="ml-1 text-sm text-white">
						{protonPhaseLabel} {state.protonSetupComponent}
						{state.protonSetupPhase === "downloading" &&
						state.protonSetupDownloadTotal > 0
							? ` (${protonDownloadedMB}MB / ${protonTotalMB}MB - ${protonSetupPct.toFixed(1)}%)`
							: state.protonSetupPhase !== "downloading"
								? "..."
								: ""}
					</h2>
					<Progressbar progress={protonSetupPct} game={game} />
				</div>
			)}
			{!isDownloading &&
				!isPaused &&
				!isAssembling &&
				!isCheckingFiles &&
				!isVerifying &&
				!isCalculatingDownloads &&
				!isSettingUpProton &&
				!isApplyingPreinstall &&
				!showDownloadBar &&
				state.downloadingGame !== null && (
					<div class="flex min-w-full flex-col gap-y-1 text-left">
						<h2 class="ml-1 text-sm text-white">Preparing...</h2>
					</div>
				)}
			{isCalculatingDownloads && state.totalFiles > 0 && (
				<div class="flex min-w-full flex-col gap-y-1 text-left">
					<h2 class="ml-1 text-sm text-white">
						Checked {formatNumber(state.checkedFiles)} of{" "}
						{formatNumber(state.totalFiles)} Files ({derived.calcPct.toFixed(2)}
						%)
					</h2>
					<Progressbar progress={derived.calcPct} game={game} />
				</div>
			)}
			{showDownloadBar && (
				<div class="flex min-w-full flex-col gap-y-1 text-left">
					<h2 class="ml-1 text-sm text-white">
						{downloadBarFinished
							? "Download finished"
							: state.downloadTotal > 0
								? derived.downloadPct >= 100
									? `Download finished - ${derived.totalGB}GB`
									: `Downloaded ${derived.downloadedGB}GB of ${derived.totalGB}GB (${derived.downloadPct.toFixed(2)}%)${derived.speedMB > 0 ? ` - ${derived.speedMB.toFixed(2)}MB/s` : ""}${derived.etaStr ? ` - ETA: ${derived.etaStr}` : ""}`
								: isFetchingManifest
									? "Fetching manifest..."
									: isCalculatingDownloads
										? "Calculating downloads..."
										: "Starting..."}
					</h2>
					<Progressbar progress={downloadBarPct} game={game} />
				</div>
			)}
			{isAssembling &&
				state.totalFiles > 0 &&
				state.checkedFiles >= state.totalFiles && (
					<div class="flex min-w-full flex-col gap-y-1 text-left">
						<h2 class="ml-1 text-sm text-white">
							Assembled {formatNumber(state.assembledFiles)} of{" "}
							{formatNumber(state.totalFiles)} chunks (
							{derived.assemblePct.toFixed(2)}%)
						</h2>
						<Progressbar progress={derived.assemblePct} game={game} />
					</div>
				)}
			{isCheckingFiles && state.totalFiles > 0 && (
				<div class="flex min-w-full flex-col gap-y-1 text-left">
					<h2 class="ml-1 text-sm text-white">
						Verified {formatNumber(state.checkedFiles)} of{" "}
						{formatNumber(state.totalFiles)} files (
						{derived.checkingPct.toFixed(2)}%)
					</h2>
					<Progressbar progress={derived.checkingPct} game={game} />
				</div>
			)}
			{isVerifying && (
				<div class="flex min-w-full flex-col gap-y-1 text-left">
					<h2 class="ml-1 text-sm text-white">
						Verified {formatNumber(state.scannedFiles)} of{" "}
						{formatNumber(state.totalFiles)} files —{" "}
						{formatNumber(state.errorCount)} errors found
					</h2>
					<Progressbar progress={derived.verifyPct} game={game} />
				</div>
			)}
			{isApplyingPreinstall && state.totalFiles > 0 && (
				<div class="flex min-w-full flex-col gap-y-1 text-left">
					<h2 class="ml-1 text-sm text-white">
						Applied {formatNumber(state.assembledFiles)} of{" "}
						{formatNumber(state.totalFiles)} files (
						{derived.applyPct.toFixed(2)}%)
					</h2>
					<Progressbar progress={derived.applyPct} game={game} />
				</div>
			)}
			{state.warningMessage && (
				<h2 class="ml-1 text-sm text-yellow-300">{state.warningMessage}</h2>
			)}
			{state.errorMessage && (
				<h2 class="ml-1 text-sm text-red-300">{state.errorMessage}</h2>
			)}
		</div>
	);
};

export default DownloadProgress;
