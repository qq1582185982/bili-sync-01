import { MediaQuery } from 'svelte/reactivity';

const MOBILE_BREAKPOINT = 768;
const TABLET_BREAKPOINT = 1024;

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
