<script lang="ts">
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import { createEventDispatcher } from 'svelte';
	import { api } from '$lib/api';
	import { toast } from 'svelte-sonner';
	import { Textarea } from '$lib/components/ui/textarea';
	import { Label } from '$lib/components/ui/label';
	import { Button } from '$lib/components/ui/button';

	export let isOpen = false;
	export let sourceName = '';
	export let sourceType = '';
	export let sourceId = 0;
	export let initialVideoPrompt = '';
	export let initialAudioPrompt = '';

	const dispatch = createEventDispatcher<{
		complete: { renamed: number; skipped: number; failed: number };
		cancel: void;
	}>();

	let videoPrompt = '';
	let audioPrompt = '';

	// 重置状态
	function resetState() {
		videoPrompt = initialVideoPrompt;
		audioPrompt = initialAudioPrompt;
	}

	// 当对话框打开时重置状态
	$: if (isOpen) {
		resetState();
	}

	// 获取视频源类型的中文名称
	function getSourceTypeLabel(type: string): string {
		const typeMap: Record<string, string> = {
			collection: '合集',
			favorite: '收藏夹',
			submission: 'UP主投稿',
			watch_later: '稍后观看'
		};
		return typeMap[type] || type;
	}

	// 执行批量重命名（异步，不阻塞界面）
	async function handleExecute() {
		// 立即关闭对话框
		isOpen = false;

		// 显示开始处理的提示
		toast.info('AI批量重命名已开始', {
			description: `正在处理 ${getSourceTypeLabel(sourceType)}「${sourceName}」，请稍候...`
		});

		// 在后台执行重命名
		executeRenameInBackground();
	}

	// 后台执行重命名逻辑
	async function executeRenameInBackground() {
		try {
			// 执行批量重命名
			const result = await api.aiRenameHistory(sourceType, sourceId);

			if (result.data.success) {
				// 重命名成功后，同步更新该源的 AI 重命名设置（包括提示词和开关）
				try {
					await api.updateVideoSourceDownloadOptions(sourceType, sourceId, {
						ai_rename: true,
						ai_rename_video_prompt: videoPrompt.trim(),
						ai_rename_audio_prompt: audioPrompt.trim()
					});
				} catch (updateError) {
					console.warn('更新AI重命名设置失败:', updateError);
				}

				toast.success('AI批量重命名完成', {
					description: `重命名 ${result.data.renamed_count} 个，跳过 ${result.data.skipped_count} 个，失败 ${result.data.failed_count} 个。已自动开启AI重命名功能并同步提示词设置。`
				});
				dispatch('complete', {
					renamed: result.data.renamed_count,
					skipped: result.data.skipped_count,
					failed: result.data.failed_count
				});
			} else {
				toast.error('批量重命名失败', { description: result.data.message });
			}
		} catch (error) {
			console.error('批量重命名失败:', error);
			toast.error('批量重命名失败', { description: (error as Error).message });
		}
	}

	function handleCancel() {
		dispatch('cancel');
		isOpen = false;
	}
</script>

<AlertDialog.Root bind:open={isOpen}>
	<AlertDialog.Content class="max-w-lg">
		<AlertDialog.Header>
			<AlertDialog.Title>AI批量重命名历史文件</AlertDialog.Title>
			<AlertDialog.Description>
				为 {getSourceTypeLabel(sourceType)} 「{sourceName}」 下的所有已下载文件执行AI重命名
			</AlertDialog.Description>
		</AlertDialog.Header>

		<div class="space-y-4 py-4">
			<!-- 视频提示词 -->
			<div class="space-y-2">
				<Label for="video-prompt">视频重命名提示词</Label>
				<Textarea
					id="video-prompt"
					bind:value={videoPrompt}
					placeholder="留空则使用全局配置的提示词..."
					rows={3}
					class="resize-none"
				/>
				<p class="text-muted-foreground text-xs">
					针对视频文件的AI重命名提示词，留空将使用全局配置
				</p>
			</div>

			<!-- 音频提示词 -->
			<div class="space-y-2">
				<Label for="audio-prompt">音频重命名提示词</Label>
				<Textarea
					id="audio-prompt"
					bind:value={audioPrompt}
					placeholder="留空则使用全局配置的提示词..."
					rows={3}
					class="resize-none"
				/>
				<p class="text-muted-foreground text-xs">
					针对音频文件的AI重命名提示词，留空将使用全局配置
				</p>
			</div>

			<!-- 提示信息 -->
			<div class="rounded-lg border border-blue-200 bg-blue-50 p-3 dark:border-blue-800 dark:bg-blue-950">
				<p class="text-xs text-blue-700 dark:text-blue-300">
					点击"开始重命名"后，对话框将关闭，任务会在后台执行。完成后将显示通知，并自动为该视频源开启AI重命名功能。
				</p>
			</div>

			<!-- 警告提示 -->
			<div class="rounded-lg border border-amber-200 bg-amber-50 p-3 dark:border-amber-800 dark:bg-amber-950">
				<p class="text-xs text-amber-700 dark:text-amber-300">
					注意：此操作会重命名该视频源下所有已下载的文件，包括视频、音频及其附属文件（NFO、字幕、封面等）。处理时间取决于文件数量和AI响应速度。
				</p>
			</div>
		</div>

		<AlertDialog.Footer>
			<AlertDialog.Cancel onclick={handleCancel}>取消</AlertDialog.Cancel>
			<Button onclick={handleExecute}>
				开始重命名
			</Button>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>
