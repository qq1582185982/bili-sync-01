import { MediaQuery } from 'svelte/reactivity';

const MOBILE_BREAKPOINT = 768;
const TABLET_BREAKPOINT = 1024;
const COMPACT_MAX_WIDTH = 1080;

export class IsMobile extends MediaQuery {
	constructor() {
		super(`max-width: ${MOBILE_BREAKPOINT - 1}px`);
	}
}

export class IsTablet extends MediaQuery {
	constructor() {
		super(`(min-width: ${MOBILE_BREAKPOINT}px) and (max-width: ${TABLET_BREAKPOINT - 1}px)`);
	}
}

/**
 * 窄屏布局：侧边栏使用抽屉，不挤占内容区宽度。
 * - true: <= 1080px
 * - false: > 1080px
 */
export class IsCompact extends MediaQuery {
	constructor() {
		super(`max-width: ${COMPACT_MAX_WIDTH}px`);
	}
}
