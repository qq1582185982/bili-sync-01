<script lang="ts">
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import { createEventDispatcher } from 'svelte';
	import { api } from '$lib/api';

	export let isOpen = false;
	export let sourceName = '';
	export let sourceType = '';
	export let sourceId = 0;
	export let initialKeywords: string[] = [];

	const dispatch = createEventDispatcher<{
		save: string[];
		cancel: void;
	}>();

	let keywords: string[] = [];
	let newKeyword = '';
	let isLoading = false;
	let isSaving = false;
	let validationError = '';
	let validationStatus: Record<number, { valid: boolean; error?: string }> = {};

	// 重置状态
	function resetState() {
		keywords = [...initialKeywords];
		newKeyword = '';
		validationError = '';
		validationStatus = {};
		isLoading = false;
		isSaving = false;
	}

	// 当对话框打开时重置状态
	$: if (isOpen) {
		resetState();
		loadKeywords();
	}

	// 加载关键词
	async function loadKeywords() {
		if (!sourceId) return;

		isLoading = true;
		try {
			const response = await api.getVideoSourceKeywordFilters(sourceType, sourceId);
			if (response.status_code === 200) {
				keywords = response.data.keyword_filters || [];
			}
		} catch (error) {
			console.error('加载关键词失败:', error);
		} finally {
			isLoading = false;
		}
	}

	// 获取视频源类型的中文名称
	function getSourceTypeLabel(type: string): string {
		const typeMap: Record<string, string> = {
			collection: '合集',
			favorite: '收藏夹',
			submission: 'UP主投稿',
			watch_later: '稍后观看',
			bangumi: '番剧'
		};
		return typeMap[type] || type;
	}

	// 验证正则表达式
	async function validatePattern(pattern: string): Promise<{ valid: boolean; error?: string }> {
		try {
			const response = await api.validateRegex(pattern);
			if (response.status_code === 200) {
				return {
					valid: response.data.valid,
					error: response.data.error
				};
			}
			return { valid: false, error: '验证请求失败' };
		} catch {
			return { valid: false, error: '网络错误' };
		}
	}

	// 添加关键词
	async function addKeyword() {
		const pattern = newKeyword.trim();
		if (!pattern) {
			validationError = '请输入关键词';
			return;
		}

		if (keywords.includes(pattern)) {
			validationError = '该关键词已存在';
			return;
		}

		// 验证正则表达式
		const result = await validatePattern(pattern);
		if (!result.valid) {
			validationError = result.error || '无效的正则表达式';
			return;
		}

		keywords = [...keywords, pattern];
		newKeyword = '';
		validationError = '';
	}

	// 删除关键词
	function removeKeyword(index: number) {
		keywords = keywords.filter((_, i) => i !== index);
		// 清除该索引的验证状态
		delete validationStatus[index];
	}

	// 保存关键词
	async function handleSave() {
		isSaving = true;
		try {
			const response = await api.updateVideoSourceKeywordFilters(sourceType, sourceId, keywords);
			if (response.status_code === 200) {
				dispatch('save', keywords);
				isOpen = false;
			} else {
				validationError = '保存失败';
			}
		} catch (error) {
			console.error('保存关键词失败:', error);
			validationError = '保存时发生错误';
		} finally {
			isSaving = false;
		}
	}

	// 处理取消
	function handleCancel() {
		if (isSaving) return;
		dispatch('cancel');
		isOpen = false;
	}

	// 处理键盘事件
	function handleKeydown(event: KeyboardEvent) {
		if (event.key === 'Enter') {
			event.preventDefault();
			addKeyword();
		}
	}
</script>

<AlertDialog.Root bind:open={isOpen}>
	<AlertDialog.Content class="max-w-lg">
		<AlertDialog.Header>
			<AlertDialog.Title class="flex items-center gap-2 text-purple-600 dark:text-purple-400">
				<svg class="h-5 w-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
					<path
						stroke-linecap="round"
						stroke-linejoin="round"
						stroke-width="2"
						d="M3 4a1 1 0 011-1h16a1 1 0 011 1v2.586a1 1 0 01-.293.707l-6.414 6.414a1 1 0 00-.293.707V17l-4 4v-6.586a1 1 0 00-.293-.707L3.293 7.293A1 1 0 013 6.586V4z"
					/>
				</svg>
				关键词过滤器
			</AlertDialog.Title>
			<AlertDialog.Description class="space-y-4">
				<div
					class="rounded-lg border border-purple-200 bg-purple-50 p-3 dark:border-purple-800 dark:bg-purple-950"
				>
					<p class="text-sm font-medium text-purple-800 dark:text-purple-200">过滤说明</p>
					<p class="mt-1 text-xs text-purple-700 dark:text-purple-300">
						匹配任一关键词的视频将被跳过，不会下载。支持正则表达式。
					</p>
				</div>

				<div class="space-y-2">
					<div class="flex items-center gap-2 text-sm">
						<span class="font-medium">类型：</span>
						<span
							class="rounded bg-purple-100 px-2 py-1 text-xs text-purple-800 dark:bg-purple-900 dark:text-purple-200"
						>
							{getSourceTypeLabel(sourceType)}
						</span>
					</div>
					<div class="flex items-center gap-2 text-sm">
						<span class="font-medium">名称：</span>
						<span class="font-mono text-gray-800 dark:text-gray-200">"{sourceName}"</span>
					</div>
				</div>

				<!-- 添加新关键词 -->
				<div class="space-y-2">
					<label for="new-keyword" class="text-sm font-medium text-gray-700 dark:text-gray-300">
						添加关键词
					</label>
					<div class="flex gap-2">
						<input
							id="new-keyword"
							type="text"
							bind:value={newKeyword}
							on:keydown={handleKeydown}
							placeholder="输入关键词或正则表达式"
							class="flex-1 rounded-md border border-gray-300 px-3 py-2 text-sm focus:border-purple-500 focus:ring-2 focus:ring-purple-500 focus:outline-none dark:border-gray-600 dark:bg-gray-700 dark:text-gray-200 dark:placeholder-gray-400 dark:focus:border-purple-400 dark:focus:ring-purple-400"
							disabled={isSaving || isLoading}
						/>
						<button
							type="button"
							class="rounded-md border border-transparent bg-purple-600 px-4 py-2 text-sm font-medium text-white hover:bg-purple-700 focus:ring-2 focus:ring-purple-500 focus:ring-offset-2 focus:outline-none disabled:cursor-not-allowed disabled:opacity-50"
							disabled={!newKeyword.trim() || isSaving || isLoading}
							on:click={addKeyword}
						>
							添加
						</button>
					</div>
					{#if validationError}
						<p class="text-xs text-red-500">{validationError}</p>
					{/if}
				</div>

				<!-- 关键词列表 -->
				<div class="space-y-2">
					<p class="text-sm font-medium text-gray-700 dark:text-gray-300">
						已添加的关键词 ({keywords.length})
					</p>
					{#if isLoading}
						<div class="flex items-center justify-center py-4">
							<svg class="h-5 w-5 animate-spin text-purple-600" fill="none" viewBox="0 0 24 24">
								<circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
								<path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
							</svg>
							<span class="ml-2 text-sm text-gray-500">加载中...</span>
						</div>
					{:else if keywords.length === 0}
						<div class="rounded-lg border border-dashed border-gray-300 bg-gray-50 p-4 text-center dark:border-gray-600 dark:bg-gray-800">
							<p class="text-sm text-gray-500 dark:text-gray-400">暂无关键词</p>
							<p class="mt-1 text-xs text-gray-400 dark:text-gray-500">添加关键词来过滤不需要的视频</p>
						</div>
					{:else}
						<div class="max-h-48 space-y-2 overflow-y-auto rounded-lg border border-gray-200 p-2 dark:border-gray-700">
							{#each keywords as keyword, index}
								<div
									class="flex items-center justify-between rounded-md bg-gray-100 px-3 py-2 dark:bg-gray-700"
								>
									<code class="flex-1 break-all text-sm text-gray-800 dark:text-gray-200">
										{keyword}
									</code>
									<button
										type="button"
										class="ml-2 flex-shrink-0 rounded p-1 text-gray-500 hover:bg-gray-200 hover:text-red-600 dark:hover:bg-gray-600 dark:hover:text-red-400"
										disabled={isSaving}
										on:click={() => removeKeyword(index)}
										title="删除"
									>
										<svg class="h-4 w-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
											<path
												stroke-linecap="round"
												stroke-linejoin="round"
												stroke-width="2"
												d="M6 18L18 6M6 6l12 12"
											/>
										</svg>
									</button>
								</div>
							{/each}
						</div>
					{/if}
				</div>

				<!-- 正则表达式示例 -->
				<div class="rounded-lg border border-gray-200 bg-gray-50 p-3 dark:border-gray-700 dark:bg-gray-800">
					<p class="text-xs font-medium text-gray-700 dark:text-gray-300">正则表达式示例：</p>
					<ul class="mt-1 space-y-1 text-xs text-gray-600 dark:text-gray-400">
						<li><code class="rounded bg-gray-200 px-1 dark:bg-gray-600">广告</code> - 匹配包含"广告"的标题</li>
						<li><code class="rounded bg-gray-200 px-1 dark:bg-gray-600">第\d+期</code> - 匹配"第N期"格式</li>
						<li><code class="rounded bg-gray-200 px-1 dark:bg-gray-600">^测试</code> - 匹配以"测试"开头的标题</li>
						<li><code class="rounded bg-gray-200 px-1 dark:bg-gray-600">预告$</code> - 匹配以"预告"结尾的标题</li>
					</ul>
				</div>
			</AlertDialog.Description>
		</AlertDialog.Header>
		<AlertDialog.Footer class="flex justify-end gap-3 pt-4">
			<button
				type="button"
				class="rounded-md border border-gray-300 bg-white px-4 py-2 text-sm font-medium text-gray-700 hover:bg-gray-50 focus:ring-2 focus:ring-indigo-500 focus:ring-offset-2 focus:outline-none disabled:cursor-not-allowed disabled:opacity-50 dark:border-gray-600 dark:bg-gray-700 dark:text-gray-200 dark:hover:bg-gray-600 dark:focus:ring-offset-gray-800"
				disabled={isSaving}
				on:click={handleCancel}
			>
				取消
			</button>
			<button
				type="button"
				class="rounded-md border border-transparent bg-purple-600 px-4 py-2 text-sm font-medium text-white hover:bg-purple-700 focus:ring-2 focus:ring-purple-500 focus:ring-offset-2 focus:outline-none disabled:cursor-not-allowed disabled:opacity-50"
				disabled={isSaving || isLoading}
				on:click={handleSave}
			>
				{#if isSaving}
					<svg class="mr-2 inline h-4 w-4 animate-spin" fill="none" viewBox="0 0 24 24">
						<circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
						<path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
					</svg>
					保存中...
				{:else}
					保存
				{/if}
			</button>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>
