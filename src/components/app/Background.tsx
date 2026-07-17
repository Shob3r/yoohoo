import { AnimatePresence, motion } from "motion/react";
import { useEffect, useRef, useState } from "preact/hooks";
import { useAedes } from "../../hooks/useAedes";
import { useGame } from "../../hooks/useGame";
// biome-ignore lint/correctness/noUnusedImports: Temporarily unused
import { Variants } from "../../types";

const BackgroundVideo = ({ src }: { src: string | null }) => {
	const ref = useRef<HTMLVideoElement>(null);
	const [visible, setVisible] = useState(false);
	const [attempt, setAttempt] = useState(0);
	const retries = useRef(0);

	useEffect(() => {
		retries.current = 0;
		setAttempt(0);

		setVisible(false);
	}, [src]);

	useEffect(() => {
		const el = ref.current;
		if (!el || !src) return;

		const onCanPlay = () => {
			el.play().catch(() => {});
			setVisible(true);
		};

		el.addEventListener("canplay", onCanPlay);
		el.src = src;
		el.load();

		return () => {
			el.removeEventListener("canplay", onCanPlay);
			el.pause();
			el.removeAttribute("src");
			el.load();
		};
	}, [src]);

	return (
		<motion.div
			class="absolute inset-0"
			animate={{ opacity: visible ? 1 : 0 }}
			transition={{ duration: 0.4 }}
		>
			<video
				ref={ref}
				key={attempt}
				class="background"
				autoPlay
				muted
				playsInline
				loop
				onError={() => {
					if (retries.current >= 5) return;
					retries.current += 1;
					setTimeout(() => setAttempt((a) => a + 1), 300);
				}}
			/>
		</motion.div>
	);
};

const BackgroundMedia = ({
	src,
	isVideo,
}: {
	src: string | null;
	isVideo: boolean;
}) => {
	if (!src) return null;
	const [attempt, setAttempt] = useState(0);
	const retries = useRef(0);

	useEffect(() => {
		retries.current = 0;
		setAttempt(0);
	}, [src]);

	return (
		<AnimatePresence mode="wait" initial={false}>
			{isVideo ? (
				<BackgroundVideo key={src} src={src} />
			) : (
				<motion.img
					key={attempt}
					alt=""
					src={src}
					class="background"
					initial={{ opacity: 0 }}
					animate={{ opacity: 1 }}
					exit={{ opacity: 0 }}
					transition={{ duration: 0.4 }}
					onError={() => {
						if (retries.current >= 5) return;
						retries.current += 1;
						setTimeout(() => setAttempt((a) => a + 1), 300);
					}}
				/>
			)}
		</AnimatePresence>
	);
};

export const Background = () => {
	const { game } = useGame();
	const { resolvedAssets } = useAedes();

	if (!resolvedAssets) return null;
	// const backgroundVideoOverlay = resolvedAssets[game].overlay;

	return (
		<div class="absolute inset-0 overflow-hidden">
			<BackgroundMedia
				src={resolvedAssets[game].backgrounds[0].image}
				isVideo={false}
			/>
			{/* Disabling for now because I've discovered that some other games can have some pre-baked overlays in some backgrounds sometimes. Will figure out later */}
			{/*game !== Variants.HK4E ? (
				<BackgroundMedia src={backgroundVideoOverlay} isVideo={false} />
			) : null*/}
		</div>
	);
};

export default Background;
