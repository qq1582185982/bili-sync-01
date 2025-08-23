<script lang="ts">
	import { onMount } from 'svelte';
	import { FileText, Download, Calendar, Clock, HardDrive, Eye } from '@lucide/svelte';
	import * as Card from '$lib/components/ui/card';
	import { Badge } from '$lib/components/ui/badge';
	import Loading from '$lib/components/ui/Loading.svelte';
	import Pagination from '$lib/components/pagination.svelte';
	import api from '$lib/api';
	import type { LiveRecord } from '$lib/types';

	export let monitorId: number;
	export let open: boolean;
	export let onClose: () => void;

	let records: LiveRecord[] = [];
	let totalCount = 0;
	let currentPage = 1;
	let pageSize = 10;
	let loading = false;
	let error = '';

	// 获取录制记录
	async function loadRecords() {
		loading = true;
		error = '';
		try {
			const response = await api.getLiveRecords(monitorId, currentPage, pageSize);
			if (response.status_code === 200) {
				records = response.data.recordings;
				totalCount = response.data.total_count;
			}
		} catch (err) {
			error = err instanceof Error ? err.message : '获取录制记录失败';
		} finally {
			loading = false;
		}
	}

	// 格式化时间
	function formatDateTime(dateString: string) {
		return new Date(dateString).toLocaleString('zh-CN');
	}

	// 格式化文件大小
	function formatFileSize(bytes?: number) {
		if (!bytes) return '未知';
		const units = ['B', 'KB', 'MB', 'GB', 'TB'];
		let size = bytes;
		let unitIndex = 0;
		
		while (size >= 1024 && unitIndex < units.length - 1) {
			size /= 1024;
			unitIndex++;
		}
		
		return `${size.toFixed(1)} ${units[unitIndex]}`;
	}

	// 计算录制时长
	function calculateDuration(startTime: string, endTime?: string) {
		if (!endTime) return '录制中';
		
		const start = new Date(startTime);
		const end = new Date(endTime);
		const diffMs = end.getTime() - start.getTime();
		
		const hours = Math.floor(diffMs / (1000 * 60 * 60));
		const minutes = Math.floor((diffMs % (1000 * 60 * 60)) / (1000 * 60));
		const seconds = Math.floor((diffMs % (1000 * 60)) / 1000);
		
		return `${hours.toString().padStart(2, '0')}:${minutes.toString().padStart(2, '0')}:${seconds.toString().padStart(2, '0')}`;
	}

	// 获取状态标签
	function getStatusBadge(status: number) {
		switch (status) {
			case 0: return { text: '未开始', variant: 'secondary' as const };
			case 1: return { text: '录制中', variant: 'default' as const };
			case 2: return { text: '已完成', variant: 'default' as const };
			case 3: return { text: '错误', variant: 'destructive' as const };
			default: return { text: '未知', variant: 'outline' as const };
		}
	}

	// 分页处理
	function handlePageChange(page: number) {
		currentPage = page;
		loadRecords();
	}

	// 页面初始化
	onMount(() => {
		if (open) {
			loadRecords();
		}
	});

	// 监听open变化
	$: if (open) {
		loadRecords();
	}
</script>

{#if open}
<div class="fixed inset-0 z-50 bg-black/80" on:click={onClose} role="presentation">
	<div class="fixed left-[50%] top-[50%] z-50 grid w-full max-w-4xl max-h-[80vh] translate-x-[-50%] translate-y-[-50%] gap-4 border bg-background p-6 shadow-lg duration-200 rounded-lg overflow-hidden flex flex-col" on:click|stopPropagation>
		<div class="flex flex-col space-y-1.5 text-center sm:text-left">
			<h2 class="text-lg font-semibold leading-none tracking-tight flex items-center gap-2">
				<FileText class="h-5 w-5" />
				录制记录
			</h2>
			<p class="text-sm text-muted-foreground">
				查看直播录制的历史记录和文件信息
			</p>
		</div>

		<div class="flex-1 overflow-auto space-y-4">
			<!-- 错误提示 -->
			{#if error}
				<div class="rounded-md bg-destructive/15 p-4 text-destructive">
					{error}
				</div>
			{/if}

			<!-- 统计信息 -->
			{#if !loading && records.length > 0}
				<div class="grid gap-4 md:grid-cols-3">
					<Card.Root>
						<Card.Header class="pb-2">
							<Card.Title class="text-sm font-medium flex items-center gap-2">
								<Eye class="h-4 w-4" />
								总录制数
							</Card.Title>
						</Card.Header>
						<Card.Content>
							<div class="text-2xl font-bold">{totalCount}</div>
						</Card.Content>
					</Card.Root>

					<Card.Root>
						<Card.Header class="pb-2">
							<Card.Title class="text-sm font-medium flex items-center gap-2">
								<Clock class="h-4 w-4" />
								总时长
							</Card.Title>
						</Card.Header>
						<Card.Content>
							<div class="text-2xl font-bold">
								{records.filter(r => r.end_time).reduce((total, record) => {
									const duration = new Date(record.end_time!).getTime() - new Date(record.start_time).getTime();
									return total + duration;
								}, 0) / (1000 * 60 * 60)}h
							</div>
						</Card.Content>
					</Card.Root>

					<Card.Root>
						<Card.Header class="pb-2">
							<Card.Title class="text-sm font-medium flex items-center gap-2">
								<HardDrive class="h-4 w-4" />
								总大小
							</Card.Title>
						</Card.Header>
						<Card.Content>
							<div class="text-2xl font-bold">
								{formatFileSize(records.reduce((total, record) => total + (record.file_size || 0), 0))}
							</div>
						</Card.Content>
					</Card.Root>
				</div>
			{/if}

			<!-- 录制记录列表 -->
			{#if loading}
				<div class="flex justify-center p-8">
					<Loading />
				</div>
			{:else if records.length === 0}
				<div class="text-center p-8 text-muted-foreground">
					<FileText class="mx-auto h-12 w-12 mb-4" />
					<p>暂无录制记录</p>
				</div>
			{:else}
				<div class="space-y-4">
					{#each records as record}
						<Card.Root>
							<Card.Content class="p-4">
								<div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
									<div>
										<h4 class="font-medium text-sm text-muted-foreground mb-1">直播标题</h4>
										<div class="max-w-xs truncate" title={record.title || '无标题'}>
											{record.title || '无标题'}
										</div>
										{#if record.file_path}
											<div class="text-xs text-muted-foreground mt-1 max-w-xs truncate" title={record.file_path}>
												{record.file_path}
											</div>
										{/if}
									</div>
									
									<div>
										<h4 class="font-medium text-sm text-muted-foreground mb-1">时间信息</h4>
										<div class="flex items-center gap-1 text-sm">
											<Calendar class="h-3 w-3" />
											{formatDateTime(record.start_time)}
										</div>
										{#if record.end_time}
											<div class="flex items-center gap-1 text-sm mt-1">
												<Calendar class="h-3 w-3" />
												{formatDateTime(record.end_time)}
											</div>
										{:else}
											<span class="text-muted-foreground text-sm">录制中</span>
										{/if}
									</div>
									
									<div>
										<h4 class="font-medium text-sm text-muted-foreground mb-1">录制信息</h4>
										<div class="flex items-center gap-1 text-sm">
											<Clock class="h-3 w-3" />
											{calculateDuration(record.start_time, record.end_time)}
										</div>
										<div class="flex items-center gap-1 text-sm mt-1">
											<HardDrive class="h-3 w-3" />
											{formatFileSize(record.file_size)}
										</div>
										<div class="mt-2">
											<Badge variant={getStatusBadge(record.status).variant}>
												{#snippet children()}
													{getStatusBadge(record.status).text}
												{/snippet}
											</Badge>
										</div>
									</div>
								</div>
							</Card.Content>
						</Card.Root>
					{/each}
				</div>

				<!-- 分页 -->
				{#if totalCount > pageSize}
					<div class="mt-4">
						<Pagination
							{currentPage}
							totalItems={totalCount}
							itemsPerPage={pageSize}
							onPageChange={handlePageChange}
						/>
					</div>
				{/if}
			{/if}
		</div>

		<div class="flex justify-end pt-4 border-t">
			<button 
				on:click={onClose}
				class="px-4 py-2 text-sm font-medium rounded-md border border-input bg-background hover:bg-accent hover:text-accent-foreground"
			>
				关闭
			</button>
		</div>
	</div>
</div>
{/if}