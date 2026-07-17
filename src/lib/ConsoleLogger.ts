import { debug, error, info, trace, warn } from "@tauri-apps/plugin-log";

const stringify = (value: unknown): string => {
	if (value instanceof Error) {
		return `${value.name}: ${value.message}\n${value.stack ?? ""}`;
	}
	if (typeof value === "string") return value;
	try {
		return JSON.stringify(value);
	} catch {
		return String(value);
	}
};

const format = (args: unknown[]): string => args.map(stringify).join(" ");

const original = {
	log: console.log.bind(console),
	info: console.info.bind(console),
	warn: console.warn.bind(console),
	error: console.error.bind(console),
	debug: console.debug.bind(console),
	trace: console.trace.bind(console),
};

let forwarding = false;
const forward = (send: (m: string) => Promise<void>, args: unknown[]): void => {
	if (forwarding) return;
	forwarding = true;
	try {
		send(format(args)).catch(() => {});
	} finally {
		forwarding = false;
	}
};

console.log = (...args: unknown[]) => {
	original.log(...args);
	forward(info, args);
};
console.info = (...args: unknown[]) => {
	original.info(...args);
	forward(info, args);
};
console.warn = (...args: unknown[]) => {
	original.warn(...args);
	forward(warn, args);
};
console.error = (...args: unknown[]) => {
	original.error(...args);
	forward(error, args);
};
console.debug = (...args: unknown[]) => {
	original.debug(...args);
	forward(debug, args);
};
console.trace = (...args: unknown[]) => {
	original.trace(...args);
	forward(trace, args);
};

window.addEventListener("unhandledrejection", (event) => {
	const reason = event.reason;
	const msg =
		reason instanceof Error
			? `${reason.name}: ${reason.message}`
			: typeof reason === "string"
				? reason
				: format([reason]);
	original.error(`Uncaught (in promise) ${msg}`);
	forward(error, [`Uncaught (in promise) ${msg}`]);
});

window.addEventListener("error", (event) => {
	const msg = `${event.message} at ${event.filename}:${event.lineno}:${event.colno}`;
	original.error(`Uncaught: ${msg}`);
	forward(error, [`Uncaught: ${msg}`]);
});
