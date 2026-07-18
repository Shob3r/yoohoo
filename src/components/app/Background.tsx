import { motion } from "motion/react";
import { useEffect, useRef, useState } from "preact/hooks";
import { useAedes } from "../../hooks/useAedes";
import { useGame } from "../../hooks/useGame";

const BackgroundVideo = ({ src }: { src: string }) => {
	const ref = useRef<HTMLVideoElement>(null);
	const [ready, setReady] = useState(false);

	useEffect(() => {
		setReady(false);
	}, [src]);

	useEffect(() => {
		const el = ref.current;
		if (!el) return;

		const onCanPlay = () => {
			el.play().catch(() => {});
			setReady(true);
		};

		el.addEventListener("canplay", onCanPlay);
		return () => {
			el.removeEventListener("canplay", onCanPlay);
			el.pause();
		};
	}, [src]);

	return (
		<motion.div
			class="absolute inset-0"
			animate={{ opacity: ready ? 1 : 0 }}
			transition={{ duration: 0.8, ease: "easeOut" }}
		>
			<video ref={ref} src={src} class="background" muted playsInline loop />
		</motion.div>
	);
};

export const Background = () => {
	const { game } = useGame();
	const { resolvedAssets } = useAedes();

	if (!resolvedAssets) return null;

	const backgrounds = resolvedAssets[game].backgrounds;
	const bg = backgrounds.find((b) => b.image && b.video) ?? backgrounds[0];

	return (
		<div class="absolute inset-0 overflow-hidden">
			<motion.img
				src={bg.image}
				alt=""
				class="background"
				initial={{ opacity: 0 }}
				animate={{ opacity: 1 }}
				transition={{ duration: 0.3 }}
			/>
			{bg.video && <BackgroundVideo src={bg.video} />}
		</div>
	);
};

export default Background;
