import { motion } from "motion/react";
import { useAedes } from "../../hooks/useAedes";
import { useGame } from "../../hooks/useGame";

export const Sidebar = () => {
	const { game, setGame } = useGame();
	const { resolvedAssets } = useAedes();

	return (
		<motion.div
			class="absolute top-0 right-4 bottom-0 z-20 flex h-auto max-h-100 w-20 flex-col items-center justify-center gap-y-4 self-center overflow-y-scroll rounded-xl bg-black/80 p-4"
		>
			{resolvedAssets &&
				Object.entries(resolvedAssets).map(([key, data]) => (
					<button
						type="button"
						key={key}
						class="relative h-12 w-12 cursor-pointer rounded-lg border-2 transition-transform duration-150 hover:scale-110 active:scale-90"
						style={{
							borderColor: game === +key ? "#fff" : "#fff0",
						}}
						onClick={() => setGame(+key)}
					>
						{data.icon && (
							<img class="absolute inset-0 rounded-lg" src={data.icon} alt="" />
						)}
					</button>
				))}
		</motion.div>
	);
};

export default Sidebar;
