import { ErrorHandler } from '$lib/error-handler';

export type LoadingSetter = (loading: boolean) => void;

export type RunRequestOptions = {
	setLoading?: LoadingSetter;
	context?: string;
	/**
	 * 自定义错误处理；提供后将跳过默认 ErrorHandler。
	 */
	onError?: (error: unknown) => void;
	/**
	 * 是否使用 ErrorHandler 弹出错误提示；默认 true。
	 */
	showErrorToast?: boolean;
};

export async function runRequest<T>(
	action: () => Promise<T>,
	{ setLoading, context, onError, showErrorToast = true }: RunRequestOptions = {}
): Promise<T | undefined> {
	setLoading?.(true);
	try {
		return await action();
	} catch (error) {
		onError?.(error);
		if (showErrorToast) {
			ErrorHandler.handleError(error, context);
		} else {
			console.error('Error occurred:', error, context ? `Context: ${context}` : '');
		}
	} finally {
		setLoading?.(false);
	}
}
