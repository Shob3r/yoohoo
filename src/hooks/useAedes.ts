import { useContext } from "preact/hooks";
import { AedesContext } from "../contexts/AedesContext";

export const useAedes = () => {
	const context = useContext(AedesContext);
	if (!context) {
		throw new Error("useAedes must be used within an AedesProvider");
	}
	return context;
};
