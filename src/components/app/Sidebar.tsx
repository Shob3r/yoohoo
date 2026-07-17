import { motion } from "motion/react";
import { useEffect, useRef, useState } from "preact/hooks";
import { useAedes } from "../../hooks/useAedes";
import { useGame } from "../../hooks/useGame";

const SidebarIcon = ({ src }: { src: string }) => {
	const [attempt, setAttempt] = useState(0);
	const retries = useRef(0);

	useEffect(() => {
		retries.current = 0;
		setAttempt(0);
	}, [src]);

	return (
		<img
			key={attempt}
			class="absolute inset-0 rounded-lg transition"
			src={src}
			alt=""
			onError={() => {
				if (retries.current >= 5) return;
				retries.current += 1;
				setTimeout(() => setAttempt((a) => a + 1), 300);
			}}
		/>
	);
};


export const Sidebar = () => {
	const { game, setGame } = useGame();
	const { resolvedAssets } = useAedes();

	return (
		<motion.div
			style={{ translateX: "-20%" }}
			class="absolute top-0 right-0 bottom-0 z-20 flex h-auto max-h-100 flex-col items-center justify-center gap-y-4 self-center overflow-y-scroll rounded-xl bg-black/80 p-4"
		>
			{resolvedAssets &&
				Object.entries(resolvedAssets).map(([key, data]) => (
					<button
						type="button"
						key={key}
						class="relative h-12 w-12 cursor-pointer rounded-lg border-white transition-transform duration-150 hover:scale-110 active:scale-90"
						style={{
							borderWidth: game === +key ? "0.125rem" : "",
						}}
						onClick={() => setGame(+key)}
					>
						<section class="bg-black/80">
							{data.icon && (
								<SidebarIcon src={data.icon}/>
							)}
						</section>
					</button>
				))}
		</motion.div>
	);
};

export default Sidebar;
