<script lang="ts">
	import { onMount } from 'svelte';
	import { Card, CardContent, CardHeader, CardTitle } from '$lib/components/ui/card/index.js';
	import { Button } from '$lib/components/ui/button/index.js';
	import { Badge } from '$lib/components/ui/badge/index.js';
	import { setBreadcrumb } from '$lib/stores/breadcrumb';
	import { toast } from 'svelte-sonner';
	import api from '$lib/api';
	import type { ApiError } from '$lib/types';
	import { VIDEO_SOURCES } from '$lib/consts';
	import { videoSourceStore, setVideoSources } from '$lib/stores/video-source';
	import DeleteVideoSourceDialog from '$lib/components/delete-video-source-dialog.svelte';
	import ResetPathDialog from '$lib/components/reset-path-dialog.svelte';
	import SubmissionSelectionDialog from '$lib/components/submission-selection-dialog.svelte';

	// 图标导入
	import PlusIcon from '@lucide/svelte/icons/plus';
	import PowerIcon from '@lucide/svelte/icons/power';
	import FolderOpenIcon from '@lucide/svelte/icons/folder-open';
	import TrashIcon from '@lucide/svelte/icons/trash-2';
	import RotateCcwIcon from '@lucide/svelte/icons/rotate-ccw';
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import ListVideoIcon from '@lucide/svelte/icons/list-video';
	import { goto } from '$app/navigation';

	let loading = false;

	// 响应式相关
	let innerWidth: number;
	let isMobile: boolean = false;
	// let isTablet: boolean = false; // 未使用，已注释
	$: isMobile = innerWidth < 768; // sm断点
	// $: isTablet = innerWidth >= 768 && innerWidth < 1024; // md断点 - 未使用

	// 折叠状态管理 - 默认所有分类都是折叠状态
	let collapsedSections: Record<string, boolean> = {};

	// 删除对话框状态
	let showDeleteDialog = false;
	let deleteSourceInfo = {
		type: '',
		id: 0,
		name: ''
	};

	// 路径重设对话框状态
	let showResetPathDialog = false;
	let resetPathSourceInfo = {
		type: '',
		id: 0,
		name: '',
		currentPath: ''
	};

	// 投稿选择对话框状态
	let showSubmissionSelectionDialog = false;
	let submissionSelectionInfo = {
		id: 0,
		upperId: 0,
		upperName: '',
		selectedVideos: [] as string[]
	};

	async function loadVideoSources() {
		loading = true;
		try {
			const response = await api.getVideoSources();
			setVideoSources(response.data);
		} catch (error) {
			console.error('加载视频源失败:', error);
			toast.error('加载视频源失败', {
				description: (error as ApiError).message
			});
		} finally {
			loading = false;
		}
	}

	// 切换视频源启用状态
	async function handleToggleEnabled(
		sourceType: string,
		sourceId: number,
		currentEnabled: boolean,
		_sourceName: string // eslint-disable-line @typescript-eslint/no-unused-vars
	) {
		try {
			const result = await api.updateVideoSourceEnabled(sourceType, sourceId, !currentEnabled);
			if (result.data.success) {
				toast.success(result.data.message);
				await loadVideoSources();
			} else {
				toast.error('操作失败', { description: result.data.message });
			}
		} catch (error: unknown) {
			console.error('切换视频源状态失败:', error);
			toast.error('操作失败', { description: error.message });
		}
	}

	// 打开删除确认对话框
	function handleDeleteSource(sourceType: string, sourceId: number, sourceName: string) {
		deleteSourceInfo = {
			type: sourceType,
			id: sourceId,
			name: sourceName
		};
		showDeleteDialog = true;
	}

	// 打开路径重设对话框
	function handleResetPath(
		sourceType: string,
		sourceId: number,
		sourceName: string,
		currentPath: string
	) {
		resetPathSourceInfo = {
			type: sourceType,
			id: sourceId,
			name: sourceName,
			currentPath: currentPath
		};
		showResetPathDialog = true;
	}

	// 切换扫描已删除视频设置
	async function handleToggleScanDeleted(
		sourceType: string,
		sourceId: number,
		currentScanDeleted: boolean
	) {
		try {
			const newScanDeleted = !currentScanDeleted;
			const result = await api.updateVideoSourceScanDeleted(sourceType, sourceId, newScanDeleted);

			if (result.data.success) {
				toast.success('设置更新成功', {
					description: result.data.message
				});
				await loadVideoSources();
			} else {
				toast.error('设置更新失败', { description: result.data.message });
			}
		} catch (error: unknown) {
			console.error('设置更新失败:', error);
			toast.error('设置更新失败', { description: error.message });
		}
	}

	// 确认删除
	async function handleConfirmDelete(event: CustomEvent<{ deleteLocalFiles: boolean }>) {
		const { deleteLocalFiles } = event.detail;

		try {
			const result = await api.deleteVideoSource(
				deleteSourceInfo.type,
				deleteSourceInfo.id,
				deleteLocalFiles
			);
			if (result.data.success) {
				toast.success('删除成功', {
					description:
						result.data.message + (deleteLocalFiles ? '，本地文件已删除' : '，本地文件已保留')
				});
				await loadVideoSources();
			} else {
				toast.error('删除失败', { description: result.data.message });
			}
		} catch (error: unknown) {
			console.error('删除视频源失败:', error);
			toast.error('删除失败', { description: error.message });
		}
	}

	// 取消删除
	function handleCancelDelete() {
		showDeleteDialog = false;
	}

	// 确认路径重设
	async function handleConfirmResetPath(
		event: CustomEvent<{
			new_path: string;
			apply_rename_rules?: boolean;
			clean_empty_folders?: boolean;
		}>
	) {
		const request = event.detail;

		try {
			const result = await api.resetVideoSourcePath(
				resetPathSourceInfo.type,
				resetPathSourceInfo.id,
				request
			);
			if (result.data.success) {
				toast.success('路径重设成功', {
					description:
						result.data.message +
						(request.apply_rename_rules ? `，已移动 ${result.data.moved_files_count} 个文件` : '')
				});
				await loadVideoSources();
			} else {
				toast.error('路径重设失败', { description: result.data.message });
			}
		} catch (error: unknown) {
			console.error('路径重设失败:', error);
			toast.error('路径重设失败', { description: error.message });
		}
	}

	// 取消路径重设
	function handleCancelResetPath() {
		showResetPathDialog = false;
	}

	// 打开投稿选择对话框
	function handleSelectSubmissionVideos(
		sourceId: number,
		upperId: number,
		upperName: string,
		selectedVideosJson: string | null
	) {
		let selectedVideos: string[] = [];
		if (selectedVideosJson) {
			try {
				selectedVideos = JSON.parse(selectedVideosJson);
			} catch (e) {
				console.error('解析选中视频列表失败:', e);
			}
		}
		submissionSelectionInfo = {
			id: sourceId,
			upperId,
			upperName,
			selectedVideos
		};
		showSubmissionSelectionDialog = true;
	}

	// 确认投稿选择
	async function handleConfirmSubmissionSelection(event: CustomEvent<string[]>) {
		const selectedVideos = event.detail;
		try {
			const result = await api.updateSubmissionSelectedVideos(
				submissionSelectionInfo.id,
				selectedVideos
			);
			if (result.data.success) {
				toast.success('历史投稿选择已更新', {
					description: result.data.message
				});
				await loadVideoSources();
			} else {
				toast.error('更新失败', { description: result.data.message });
			}
		} catch (error: unknown) {
			console.error('更新投稿选择失败:', error);
			toast.error('更新失败', { description: (error as Error).message });
		}
	}

	// 取消投稿选择
	function handleCancelSubmissionSelection() {
		showSubmissionSelectionDialog = false;
	}

	// 切换折叠状态
	function toggleCollapse(sectionKey: string) {
		// 如果未设置，默认为折叠状态(true)，点击后变为展开状态(false)
		// 如果已设置，则切换状态
		if (collapsedSections[sectionKey] === undefined) {
			collapsedSections[sectionKey] = false; // 第一次点击展开
		} else {
			collapsedSections[sectionKey] = !collapsedSections[sectionKey];
		}
		collapsedSections = { ...collapsedSections };
	}

	function navigateToAddSource() {
		goto('/add-source');
	}

	onMount(() => {
		setBreadcrumb([{ label: '视频源管理' }]);
		loadVideoSources();
	});
</script>

<svelte:head>
	<title>视频源管理 - Bili Sync</title>
</svelte:head>

<svelte:window bind:innerWidth />

<div class="space-y-6">
	<!-- 页面头部 -->
	<div class="flex {isMobile ? 'flex-col gap-4' : 'flex-row items-center justify-between gap-4'}">
		<div>
			<h1 class="{isMobile ? 'text-xl' : 'text-2xl'} font-bold">视频源管理</h1>
			<p class="{isMobile ? 'text-sm' : 'text-base'} text-muted-foreground">
				管理和配置您的视频源，包括收藏夹、合集、投稿和稍后再看
			</p>
		</div>
		<Button
			onclick={navigateToAddSource}
			class="flex items-center gap-2 {isMobile ? 'w-full' : 'w-auto'}"
		>
			<PlusIcon class="h-4 w-4" />
			添加视频源
		</Button>
	</div>

	{#if loading}
		<div class="flex items-center justify-center py-12">
			<div class="text-muted-foreground">加载中...</div>
		</div>
	{:else}
		<!-- 视频源分类展示 -->
		<div class="grid gap-6">
			{#each Object.entries(VIDEO_SOURCES) as [sourceKey, sourceConfig] (sourceKey)}
				{@const sources = $videoSourceStore ? $videoSourceStore[sourceConfig.type] : []}
				<Card>
					<CardHeader class="cursor-pointer" onclick={() => toggleCollapse(sourceKey)}>
						<CardTitle class="flex items-center gap-2">
							{#if collapsedSections[sourceKey] !== false}
								<ChevronRightIcon class="text-muted-foreground h-4 w-4" />
							{:else}
								<ChevronDownIcon class="text-muted-foreground h-4 w-4" />
							{/if}
							<sourceConfig.icon class="h-5 w-5" />
							{sourceConfig.title}
							<Badge variant="outline" class="ml-auto">
								{sources?.length || 0} 个
							</Badge>
						</CardTitle>
					</CardHeader>
					{#if collapsedSections[sourceKey] === false}
						<CardContent>
							{#if sources && sources.length > 0}
								<div class="space-y-3">
									{#each sources as source (source.id)}
										<div
											class="flex {isMobile
												? 'flex-col gap-3'
												: 'flex-row items-center justify-between gap-3'} rounded-lg border p-3"
										>
											<div class="min-w-0 flex-1">
												<div
													class="flex {isMobile
														? 'flex-col gap-2'
														: 'flex-row items-center gap-2'} mb-1"
												>
													<span class="truncate font-medium">{source.name}</span>
													<Badge
														variant={source.enabled ? 'default' : 'secondary'}
														class="w-fit text-xs"
													>
														{source.enabled ? '已启用' : '已禁用'}
													</Badge>
												</div>
												<div class="text-muted-foreground truncate text-sm" title={source.path}>
													{source.path || '未设置路径'}
												</div>
												<!-- 显示对应类型的ID -->
												<div class="text-muted-foreground mt-1 text-xs">
													{#if sourceConfig.type === 'favorite' && source.f_id}
														收藏夹ID: {source.f_id}
													{:else if sourceConfig.type === 'collection' && source.s_id}
														合集ID: {source.s_id}
														{#if source.m_id}
															| UP主ID: {source.m_id}{/if}
													{:else if sourceConfig.type === 'submission' && source.upper_id}
														UP主ID: {source.upper_id}
														{#if source.selected_videos}
															{@const selectedCount = (() => {
																try {
																	return JSON.parse(source.selected_videos).length;
																} catch {
																	return 0;
																}
															})()}
															{#if selectedCount > 0}
																<span class="ml-2 text-purple-600">| 已选 {selectedCount} 个历史投稿</span>
															{/if}
														{/if}
													{:else if sourceConfig.type === 'bangumi'}
														{#if source.season_id}<span class="block"
																>主季度ID: {source.season_id}</span
															>{/if}
														{#if source.selected_seasons?.length}
															<span class="block"
																>已选季度ID: {source.selected_seasons.join(', ')}</span
															>
														{/if}
														{#if source.media_id}<span class="block"
																>Media ID: {source.media_id}</span
															>{/if}
													{:else if sourceConfig.type === 'watch_later'}
														稍后再看 (无特定ID)
													{/if}
												</div>
												{#if source.scan_deleted_videos}
													<div class="mt-1 text-xs text-blue-600">扫描删除视频已启用</div>
												{/if}
											</div>

											<div class="flex items-center justify-end gap-1 sm:ml-4">
												<!-- 启用/禁用 -->
												<Button
													size="sm"
													variant="ghost"
													onclick={() =>
														handleToggleEnabled(
															sourceConfig.type,
															source.id,
															source.enabled,
															source.name
														)}
													title={source.enabled ? '禁用' : '启用'}
													class="h-8 w-8 p-0"
												>
													<PowerIcon
														class="h-4 w-4 {source.enabled ? 'text-green-600' : 'text-gray-400'}"
													/>
												</Button>

												<!-- 选择历史投稿（仅投稿类型显示） -->
												{#if sourceConfig.type === 'submission'}
													<Button
														size="sm"
														variant="ghost"
														onclick={() =>
															handleSelectSubmissionVideos(
																source.id,
																source.upper_id,
																source.name,
																source.selected_videos
															)}
														title="选择历史投稿"
														class="h-8 w-8 p-0"
													>
														<ListVideoIcon class="h-4 w-4 text-purple-600" />
													</Button>
												{/if}

												<!-- 重设路径 -->
												<Button
													size="sm"
													variant="ghost"
													onclick={() =>
														handleResetPath(sourceConfig.type, source.id, source.name, source.path)}
													title="重设路径"
													class="h-8 w-8 p-0"
												>
													<FolderOpenIcon class="h-4 w-4 text-orange-600" />
												</Button>

												<!-- 扫描删除视频设置 -->
												<Button
													size="sm"
													variant="ghost"
													onclick={() =>
														handleToggleScanDeleted(
															sourceConfig.type,
															source.id,
															source.scan_deleted_videos
														)}
													title={source.scan_deleted_videos ? '禁用扫描已删除' : '启用扫描已删除'}
													class="h-8 w-8 p-0"
												>
													<RotateCcwIcon
														class="h-4 w-4 {source.scan_deleted_videos
															? 'text-blue-600'
															: 'text-gray-400'}"
													/>
												</Button>

												<!-- 删除 -->
												<Button
													size="sm"
													variant="ghost"
													onclick={() =>
														handleDeleteSource(sourceConfig.type, source.id, source.name)}
													title="删除"
													class="h-8 w-8 p-0"
												>
													<TrashIcon class="text-destructive h-4 w-4" />
												</Button>
											</div>
										</div>
									{/each}
								</div>
							{:else}
								<div class="flex flex-col items-center justify-center py-8 text-center">
									<sourceConfig.icon class="text-muted-foreground mb-4 h-12 w-12" />
									<div class="text-muted-foreground mb-2">暂无{sourceConfig.title}</div>
									<p class="text-muted-foreground mb-4 text-sm">
										{#if sourceConfig.type === 'favorite'}
											还没有添加任何收藏夹订阅
										{:else if sourceConfig.type === 'collection'}
											还没有添加任何合集或列表订阅
										{:else if sourceConfig.type === 'submission'}
											还没有添加任何用户投稿订阅
										{:else}
											还没有添加稍后再看订阅
										{/if}
									</p>
									<Button size="sm" variant="outline" onclick={navigateToAddSource}>
										<PlusIcon class="mr-2 h-4 w-4" />
										添加{sourceConfig.title}
									</Button>
								</div>
							{/if}
						</CardContent>
					{/if}
				</Card>
			{/each}
		</div>
	{/if}
</div>

<!-- 删除确认对话框 -->
<DeleteVideoSourceDialog
	bind:isOpen={showDeleteDialog}
	sourceName={deleteSourceInfo.name}
	sourceType={deleteSourceInfo.type}
	on:confirm={handleConfirmDelete}
	on:cancel={handleCancelDelete}
/>

<!-- 路径重设对话框 -->
<ResetPathDialog
	bind:isOpen={showResetPathDialog}
	sourceName={resetPathSourceInfo.name}
	sourceType={resetPathSourceInfo.type}
	currentPath={resetPathSourceInfo.currentPath}
	on:confirm={handleConfirmResetPath}
	on:cancel={handleCancelResetPath}
/>

<!-- 投稿选择对话框 -->
<SubmissionSelectionDialog
	bind:isOpen={showSubmissionSelectionDialog}
	sourceId={submissionSelectionInfo.id}
	upperId={submissionSelectionInfo.upperId}
	upperName={submissionSelectionInfo.upperName}
	initialSelectedVideos={submissionSelectionInfo.selectedVideos}
	on:confirm={handleConfirmSubmissionSelection}
	on:cancel={handleCancelSubmissionSelection}
/>
