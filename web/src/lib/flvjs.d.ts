declare module 'flv.js' {
	const flvjs: {
		isSupported?: () => boolean;
		createPlayer?: (mediaDataSource: unknown, config?: unknown) => unknown;
	};
	export default flvjs;
}
