<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import { Card, CardContent, CardHeader, CardTitle } from '$lib/components/ui/card/index.js';
	import { Progress } from '$lib/components/ui/progress/index.js';
	import { Badge } from '$lib/components/ui/badge/index.js';
	import { Button } from '$lib/components/ui/button/index.js';
	import * as Chart from '$lib/components/ui/chart/index.js';
	import MyChartTooltip from '$lib/components/custom/my-chart-tooltip.svelte';
	import { curveNatural } from 'd3-shape';
	import { BarChart, AreaChart } from 'layerchart';
	import { setBreadcrumb } from '$lib/stores/breadcrumb';
	import { toast } from 'svelte-sonner';
	import api from '$lib/api';
	import { wsManager } from '$lib/ws';
	import type {
		DashBoardResponse,
		SysInfo,
		ApiError,
		TaskStatus,
		TaskControlStatusResponse,
		LatestIngestItem
	} from '$lib/types';
	import AuthLogin from '$lib/components/auth-login.svelte';
	import InitialSetup from '$lib/components/initial-setup.svelte';

	// 图标导入
	import CloudDownloadIcon from '@lucide/svelte/icons/cloud-download';
	import DatabaseIcon from '@lucide/svelte/icons/database';
	import HeartIcon from '@lucide/svelte/icons/heart';
	import FolderIcon from '@lucide/svelte/icons/folder';
	import UserIcon from '@lucide/svelte/icons/user';
	import ClockIcon from '@lucide/svelte/icons/clock';
	import VideoIcon from '@lucide/svelte/icons/video';
	import TvIcon from '@lucide/svelte/icons/tv';
	import HardDriveIcon from '@lucide/svelte/icons/hard-drive';
	import CpuIcon from '@lucide/svelte/icons/cpu';
	import MemoryStickIcon from '@lucide/svelte/icons/memory-stick';
	import PlayIcon from '@lucide/svelte/icons/play';
	import CheckCircleIcon from '@lucide/svelte/icons/check-circle';
	import CalendarIcon from '@lucide/svelte/icons/calendar';
	import PauseIcon from '@lucide/svelte/icons/pause';
	import SettingsIcon from '@lucide/svelte/icons/settings';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import XCircleIcon from '@lucide/svelte/icons/x-circle';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';

	// 认证状态
	let isAuthenticated = false;
	let needsInitialSetup = false;
	let checkingSetup = true;

	let dashboardData: DashBoardResponse | null = null;
	let sysInfo: SysInfo | null = null;
	let taskStatus: TaskStatus | null = null;
	let taskControlStatus: TaskControlStatusResponse | null = null;
	let latestIngests: LatestIngestItem[] = [];
	let loading = false;
	let loadingTaskControl = false;
	let loadingLatestIngests = false;
	let loadingTaskRefresh = false;
	let unsubscribeSysInfo: (() => void) | null = null;
	let unsubscribeTasks: (() => void) | null = null;

	function formatBytes(bytes: number): string {
		if (bytes === 0) return '0 B';
		const k = 1024;
		const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
		const i = Math.floor(Math.log(bytes) / Math.log(k));
		return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i];
	}

	function formatCpu(cpu: number): string {
		return `${cpu.toFixed(1)}%`;
	}

	function formatSpeed(bps: number | null): string {
		if (!bps || bps <= 0) return '-';
		const mbps = bps / 1024 / 1024;
		if (mbps >= 1) return `${mbps.toFixed(2)} MB/s`;
		const kbps = bps / 1024;
		return `${kbps.toFixed(0)} KB/s`;
	}

	// 格式化时间为 HH:MM:SS AM/PM 格式
	function formatTime(timeStr: string | null | undefined): string {
		if (!timeStr) return '-';
		try {
			const date = new Date(timeStr);
			if (isNaN(date.getTime())) return timeStr;
			return date.toLocaleString('en-US', {
				hour: '2-digit',
				minute: '2-digit',
				second: '2-digit',
				hour12: true
			});
		} catch {
			return timeStr;
		}
	}

	// 处理登录成功
	function handleLoginSuccess() {
		isAuthenticated = true;
		loadInitialData();
	}

	// 处理初始设置完成
	function handleSetupComplete() {
		needsInitialSetup = false;
		checkingSetup = true;
		checkInitialSetup().then(() => {
			if (isAuthenticated) {
				window.dispatchEvent(new CustomEvent('login-success'));
			}
		});
	}

	// 检查是否需要初始设置
	async function checkInitialSetup() {
		try {
			const storedToken = localStorage.getItem('auth_token');

			if (!storedToken) {
				try {
					const setupCheck = await api.checkInitialSetup();
					if (setupCheck.data.needs_setup) {
						needsInitialSetup = true;
					} else {
						needsInitialSetup = false;
						isAuthenticated = false;
					}
				} catch {
					console.log('无法检查后端状态，显示初始设置');
					needsInitialSetup = true;
				}
				checkingSetup = false;
				return;
			}

			api.setAuthToken(storedToken);
			try {
				await api.getVideoSources();
				isAuthenticated = true;
				loadInitialData();
			} catch {
				localStorage.removeItem('auth_token');
				api.setAuthToken('');

				try {
					const setupCheck = await api.checkInitialSetup();
					if (setupCheck.data.needs_setup) {
						needsInitialSetup = true;
					} else {
						needsInitialSetup = false;
						isAuthenticated = false;
					}
				} catch {
					needsInitialSetup = false;
					isAuthenticated = false;
				}
			}
		} catch (error) {
			console.error('检查初始设置失败:', error);
			needsInitialSetup = false;
			isAuthenticated = false;
		} finally {
			checkingSetup = false;
		}
	}

	async function loadDashboard() {
		loading = true;
		try {
			const response = await api.getDashboard();
			dashboardData = response.data;
		} catch (error) {
			console.error('加载仪表盘数据失败:', error);
			toast.error('加载仪表盘数据失败', {
				description: (error as ApiError).message
			});
		} finally {
			loading = false;
		}
	}

	async function loadLatestIngests() {
		loadingLatestIngests = true;
		try {
			const response = await api.getLatestIngests(10);
			latestIngests = response.data.items || [];
		} catch (error) {
			console.error('加载最新入库失败:', error);
			toast.error('加载最新入库失败', {
				description: (error as ApiError).message
			});
		} finally {
			loadingLatestIngests = false;
		}
	}

	// 加载任务控制状态
	async function loadTaskControlStatus() {
		try {
			const response = await api.getTaskControlStatus();
			taskControlStatus = response.data;
		} catch (error) {
			console.error('获取任务控制状态失败:', error);
		}
	}

	// 暂停所有任务
	async function pauseAllTasks() {
		if (loadingTaskControl) return;

		loadingTaskControl = true;
		try {
			const response = await api.pauseScanning();
			if (response.data.success) {
				toast.success(response.data.message);
				await loadTaskControlStatus();
			} else {
				toast.error('暂停任务失败');
			}
		} catch (error) {
			console.error('暂停任务失败:', error);
			toast.error('暂停任务失败');
		} finally {
			loadingTaskControl = false;
		}
	}

	// 恢复所有任务
	async function resumeAllTasks() {
		if (loadingTaskControl) return;

		loadingTaskControl = true;
		try {
			const response = await api.resumeScanning();
			if (response.data.success) {
				toast.success(response.data.message);
				await loadTaskControlStatus();
			} else {
				toast.error('恢复任务失败');
			}
		} catch (error) {
			console.error('恢复任务失败:', error);
			toast.error('恢复任务失败');
		} finally {
			loadingTaskControl = false;
		}
	}

	// 任务刷新（触发立即扫描/下载，不等待下一次定时触发）
	async function refreshTasks() {
		if (loadingTaskRefresh) return;

		loadingTaskRefresh = true;
		try {
			const response = await api.refreshScanning();
			if (response.data.success) {
				toast.success(response.data.message);
				await loadTaskControlStatus();
				// 刷新后同步刷新首页数据
				await loadDashboard();
				await loadLatestIngests();
			} else {
				toast.error('任务刷新失败');
			}
		} catch (error) {
			console.error('任务刷新失败:', error);
			toast.error('任务刷新失败');
		} finally {
			loadingTaskRefresh = false;
		}
	}

	async function loadInitialData() {
		try {
			// 加载任务控制状态
			await loadTaskControlStatus();
			// 加载仪表盘数据
			await loadDashboard();
			// 加载最新入库
			await loadLatestIngests();
		} catch (error) {
			console.error('加载数据失败:', error);
			toast.error('加载数据失败');
		}
	}

	onMount(() => {
		setBreadcrumb([{ label: '首页' }]);

		// 订阅WebSocket事件
		unsubscribeSysInfo = wsManager.subscribeToSysInfo((data) => {
			sysInfo = data;
		});
		unsubscribeTasks = wsManager.subscribeToTasks((data: TaskStatus) => {
			taskStatus = data;
		});

		// 检查认证状态
		checkInitialSetup();

		// 连接WebSocket
		wsManager.connect().catch((error) => {
			console.error('WebSocket连接失败:', error);
		});
	});

	onDestroy(() => {
		if (unsubscribeSysInfo) {
			unsubscribeSysInfo();
			unsubscribeSysInfo = null;
		}
		if (unsubscribeTasks) {
			unsubscribeTasks();
			unsubscribeTasks = null;
		}
	});

	let memoryHistory: Array<{ time: Date; used: number; process: number }> = [];
	let cpuHistory: Array<{ time: Date; used: number; process: number }> = [];

	$: if (sysInfo) {
		memoryHistory = [
			...memoryHistory.slice(-19),
			{
				time: new Date(),
				used: sysInfo.used_memory,
				process: sysInfo.process_memory
			}
		];
		cpuHistory = [
			...cpuHistory.slice(-19),
			{
				time: new Date(),
				used: sysInfo.used_cpu,
				process: sysInfo.process_cpu
			}
		];
	}

	// 计算磁盘使用率
	$: diskUsagePercent = sysInfo
		? ((sysInfo.total_disk - sysInfo.available_disk) / sysInfo.total_disk) * 100
		: 0;

	// 图表配置
	const videoChartConfig = {
		videos: {
			label: '视频数量',
			color: 'var(--color-slate-700)'
		}
	} satisfies Chart.ChartConfig;

	const memoryChartConfig = {
		used: {
			label: '整体占用',
			color: 'var(--color-slate-700)'
		},
		process: {
			label: '程序占用',
			color: 'var(--color-slate-950)'
		}
	} satisfies Chart.ChartConfig;

	const cpuChartConfig = {
		used: {
			label: '整体占用',
			color: 'var(--color-slate-700)'
		},
		process: {
			label: '程序占用',
			color: 'var(--color-slate-950)'
		}
	} satisfies Chart.ChartConfig;
</script>

<svelte:head>
	<title>首页 - Bili Sync</title>
</svelte:head>

{#if checkingSetup}
	<div class="bg-background flex min-h-screen items-center justify-center">
		<div class="text-center">
			<div class="text-foreground mb-4 text-lg">正在检查系统状态...</div>
			<div class="text-muted-foreground text-sm">请稍候</div>
		</div>
	</div>
{:else if needsInitialSetup}
	<InitialSetup on:setup-complete={handleSetupComplete} />
{:else if !isAuthenticated}
	<AuthLogin on:login-success={handleLoginSuccess} />
{:else}
	<div class="space-y-6">
		{#if loading}
			<div class="flex items-center justify-center py-12">
				<div class="text-muted-foreground">加载中...</div>
			</div>
		{:else}
			<!-- 第一行：存储空间 + 当前监听 -->
			<div class="grid gap-4 md:grid-cols-3">
				<Card class="md:col-span-1">
					<CardHeader class="flex flex-row items-center justify-between space-y-0 pb-2">
						<CardTitle class="text-sm font-medium">存储空间</CardTitle>
						<HardDriveIcon class="text-muted-foreground h-4 w-4" />
					</CardHeader>
					<CardContent>
						{#if sysInfo}
							<div class="space-y-2">
								<div class="flex items-center justify-between">
									<div class="text-2xl font-bold">{formatBytes(sysInfo.available_disk)} 可用</div>
									<div class="text-muted-foreground text-sm">
										共 {formatBytes(sysInfo.total_disk)}
									</div>
								</div>
								<Progress value={diskUsagePercent} class="h-2" />
								<div class="text-muted-foreground text-xs">
									已使用 {diskUsagePercent.toFixed(1)}% 的存储空间
								</div>
							</div>
						{:else}
							<div class="text-muted-foreground text-sm">加载中...</div>
						{/if}
					</CardContent>
				</Card>
				<Card class="md:col-span-2">
					<CardHeader class="flex flex-row items-center justify-between space-y-0 pb-2">
						<CardTitle class="text-sm font-medium">当前监听</CardTitle>
						<DatabaseIcon class="text-muted-foreground h-4 w-4" />
					</CardHeader>
					<CardContent>
						{#if dashboardData}
						<div class="space-y-4">
							<!-- 监听状态 -->
							<div class="flex flex-col gap-2 md:flex-row md:items-center md:justify-between">
								<div class="flex items-center gap-2">
									{#if taskControlStatus && taskControlStatus.is_paused}
										<Badge variant="destructive">已停止</Badge>
									{:else if dashboardData.monitoring_status.is_scanning}
										<Badge>扫描中</Badge>
									{:else}
										<Badge variant="outline">等待中</Badge>
									{/if}
								</div>
								<div class="flex items-center gap-2">
									<div class="text-muted-foreground text-sm">下次扫描</div>
									<div class="text-sm font-medium">
										{formatTime(dashboardData.monitoring_status.next_scan_time)}
									</div>
									<Button
										size="sm"
										variant="outline"
										onclick={() => {
											loadDashboard();
											loadLatestIngests();
										}}
										class="h-8"
										title="刷新首页数据"
									>
										<RefreshCwIcon class="mr-2 h-4 w-4" />
										刷新
									</Button>
								</div>
							</div>

							<!-- 扫描摘要 -->
							<div class="grid grid-cols-1 gap-3 md:grid-cols-3">
								<div class="flex items-center justify-between">
									<span class="text-sm">监听源</span>
									<Badge variant="outline">
										{dashboardData.monitoring_status.active_sources} / {dashboardData.monitoring_status.total_sources}
									</Badge>
								</div>
								<div class="flex items-center justify-between">
									<span class="text-sm">上次扫描</span>
									<span class="text-muted-foreground text-sm">
										{formatTime(dashboardData.monitoring_status.last_scan_time)}
									</span>
								</div>
								<div class="flex items-center justify-between">
									<span class="text-sm">未启用</span>
									<span class="text-muted-foreground text-sm">
										{dashboardData.monitoring_status.inactive_sources}
									</span>
								</div>
							</div>

							<!-- 具体监听项统计 -->
							<div class="grid grid-cols-2 gap-4 md:grid-cols-3">
									<div class="flex items-center justify-between">
										<div class="flex items-center gap-2">
											<HeartIcon class="text-muted-foreground h-4 w-4" />
											<span class="text-sm">收藏夹</span>
										</div>
										<Badge variant="outline"
											>{dashboardData.enabled_favorites} / {dashboardData.total_favorites}</Badge
										>
									</div>
									<div class="flex items-center justify-between">
										<div class="flex items-center gap-2">
											<FolderIcon class="text-muted-foreground h-4 w-4" />
											<span class="text-sm">合集 / 列表</span>
										</div>
										<Badge variant="outline"
											>{dashboardData.enabled_collections} / {dashboardData.total_collections}</Badge
										>
									</div>
									<div class="flex items-center justify-between">
										<div class="flex items-center gap-2">
											<UserIcon class="text-muted-foreground h-4 w-4" />
											<span class="text-sm">投稿</span>
										</div>
										<Badge variant="outline"
											>{dashboardData.enabled_submissions} / {dashboardData.total_submissions}</Badge
										>
									</div>
									<div class="flex items-center justify-between">
										<div class="flex items-center gap-2">
											<ClockIcon class="text-muted-foreground h-4 w-4" />
											<span class="text-sm">稍后再看</span>
										</div>
										<Badge variant="outline">
											{dashboardData.enable_watch_later
												? `启用 (${dashboardData.total_watch_later})`
												: `禁用 (${dashboardData.total_watch_later})`}
										</Badge>
									</div>
									<div class="flex items-center justify-between">
										<div class="flex items-center gap-2">
											<TvIcon class="text-muted-foreground h-4 w-4" />
											<span class="text-sm">番剧</span>
										</div>
										<Badge variant="outline"
											>{dashboardData.enabled_bangumi} / {dashboardData.total_bangumi}</Badge
										>
									</div>
								</div>
							</div>
						{:else}
							<div class="text-muted-foreground text-sm">加载中...</div>
						{/if}
					</CardContent>
				</Card>
			</div>

			<!-- 第二行：最近入库 + 下载任务状态 -->
			<div class="grid gap-4 md:grid-cols-3">
				<Card class="max-w-full overflow-hidden md:col-span-2">
					<CardHeader class="flex flex-row items-center justify-between space-y-0 pb-2">
						<CardTitle class="text-sm font-medium">最近入库</CardTitle>
						<VideoIcon class="text-muted-foreground h-4 w-4" />
					</CardHeader>
					<CardContent>
						{#if dashboardData && dashboardData.videos_by_day.length > 0}
							<div class="mb-4 space-y-2">
								<div class="flex items-center justify-between text-sm">
									<span>近七日新增视频</span>
									<span class="font-medium"
										>{dashboardData.videos_by_day.reduce((sum, v) => sum + v.cnt, 0)} 个</span
									>
								</div>
							</div>
							<Chart.Container config={videoChartConfig} class="h-[200px] w-full">
								<BarChart
									data={dashboardData.videos_by_day}
									x="day"
									axis="x"
									series={[
										{
											key: 'cnt',
											label: '新增视频',
											color: videoChartConfig.videos.color
										}
									]}
									props={{
										bars: {
											stroke: 'none',
											rounded: 'all',
											radius: 8,
											initialHeight: 0
										},
										highlight: { area: { fill: 'none' } },
										xAxis: { format: () => '' }
									}}
								>
									{#snippet tooltip()}
										<MyChartTooltip indicator="line" />
									{/snippet}
								</BarChart>
							</Chart.Container>
						{:else}
							<div class="text-muted-foreground flex h-[200px] items-center justify-center text-sm">
								暂无视频统计数据
							</div>
						{/if}

						<!-- 最新入库列表 -->
						<div class="mt-6 space-y-3">
							<div class="flex items-center justify-between">
								<span class="text-sm font-medium">最新入库</span>
								<Button
									size="sm"
									variant="outline"
									onclick={() => loadLatestIngests()}
									disabled={loadingLatestIngests}
									class="h-8"
									title="刷新最新入库"
								>
									{#if loadingLatestIngests}
										<SettingsIcon class="mr-2 h-4 w-4 animate-spin" />
										加载中...
									{:else}
										<RefreshCwIcon class="mr-2 h-4 w-4" />
										刷新
									{/if}
								</Button>
							</div>

							<div class="overflow-x-auto rounded-md border">
								<table class="w-full text-sm">
									<thead class="bg-muted/50">
										<tr>
											<th class="px-3 py-2 text-left font-medium">视频</th>
											<th class="px-3 py-2 text-left font-medium">作者</th>
											<th class="px-3 py-2 text-left font-medium">位置</th>
											<th class="px-3 py-2 text-left font-medium">入库时间</th>
											<th class="px-3 py-2 text-left font-medium">速度</th>
											<th class="px-3 py-2 text-left font-medium">结果</th>
										</tr>
									</thead>
									<tbody>
										{#if latestIngests.length === 0}
											<tr>
												<td class="text-muted-foreground px-3 py-6" colspan="6">暂无入库记录</td>
											</tr>
										{:else}
											{#each latestIngests as item (item.video_id)}
												<tr class="border-t">
													<td class="px-3 py-2">
														<div class="max-w-[260px] truncate" title={item.video_name}>
															{item.video_name}
														</div>
													</td>
													<td class="px-3 py-2">
														<div class="max-w-[160px] truncate" title={item.upper_name}>
															{item.upper_name}
														</div>
													</td>
													<td class="px-3 py-2">
														<div class="max-w-[320px] truncate" title={item.path}>{item.path}</div>
													</td>
													<td class="px-3 py-2 whitespace-nowrap">{item.ingested_at}</td>
													<td class="px-3 py-2 whitespace-nowrap">{formatSpeed(item.download_speed_bps)}</td>
													<td class="px-3 py-2">
														{#if item.status === 'success'}
															<div class="flex items-center gap-1 text-emerald-600">
																<CheckCircleIcon class="h-4 w-4" />
																成功
															</div>
														{:else if item.status === 'deleted'}
															<div class="flex items-center gap-1 text-amber-600">
																<Trash2Icon class="h-4 w-4" />
																已删除
															</div>
														{:else}
															<div class="flex items-center gap-1 text-rose-600">
																<XCircleIcon class="h-4 w-4" />
																失败
															</div>
														{/if}
													</td>
												</tr>
											{/each}
										{/if}
									</tbody>
								</table>
							</div>
						</div>
					</CardContent>
				</Card>
				<Card class="max-w-full md:col-span-1">
					<CardHeader class="flex flex-row items-center justify-between space-y-0 pb-2">
						<CardTitle class="text-sm font-medium">下载任务状态</CardTitle>
						<CloudDownloadIcon class="text-muted-foreground h-4 w-4" />
					</CardHeader>
					<CardContent>
						{#if taskStatus}
							<div class="space-y-4">
								<div class="grid grid-cols-1 gap-6">
									<div class="mb-4 space-y-2">
										<div class="flex items-center justify-between text-sm">
											<span>当前任务状态</span>
											{#if taskControlStatus && taskControlStatus.is_paused}
												<Badge variant="destructive">已停止</Badge>
											{:else if taskStatus.is_running}
												<Badge>扫描中</Badge>
											{:else}
												<Badge variant="outline">等待中</Badge>
											{/if}
										</div>
									</div>
									<div class="flex items-center justify-between">
										<div class="flex items-center gap-2">
											<PlayIcon class="text-muted-foreground h-4 w-4" />
											<span class="text-sm">开始运行</span>
										</div>
										<span class="text-muted-foreground text-sm">
											{taskStatus.last_run
												? new Date(taskStatus.last_run).toLocaleString('en-US', {
														hour: '2-digit',
														minute: '2-digit',
														second: '2-digit',
														hour12: true
													})
												: '-'}
										</span>
									</div>
									<div class="flex items-center justify-between">
										<div class="flex items-center gap-2">
											<CheckCircleIcon class="text-muted-foreground h-4 w-4" />
											<span class="text-sm">运行结束</span>
										</div>
										<span class="text-muted-foreground text-sm">
											{taskStatus.last_finish
												? new Date(taskStatus.last_finish).toLocaleString('en-US', {
														hour: '2-digit',
														minute: '2-digit',
														second: '2-digit',
														hour12: true
													})
												: '-'}
										</span>
									</div>
									<div class="flex items-center justify-between">
										<div class="flex items-center gap-2">
											<CalendarIcon class="text-muted-foreground h-4 w-4" />
											<span class="text-sm">下次运行</span>
										</div>
										<span class="text-muted-foreground text-sm">
											{taskStatus.next_run
												? new Date(taskStatus.next_run).toLocaleString('en-US', {
														hour: '2-digit',
														minute: '2-digit',
														second: '2-digit',
														hour12: true
													})
												: '-'}
										</span>
									</div>
								</div>

								<!-- 任务控制按钮 -->
								{#if taskControlStatus}
									<div class="grid grid-cols-2 gap-2">
										<Button
											size="sm"
											variant={taskControlStatus.is_paused ? 'default' : 'destructive'}
											onclick={taskControlStatus.is_paused ? resumeAllTasks : pauseAllTasks}
											disabled={loadingTaskControl}
											class="w-full"
											title={taskControlStatus.is_paused
												? '恢复所有下载和扫描任务'
												: '停止所有下载和扫描任务'}
										>
											{#if loadingTaskControl}
												<SettingsIcon class="mr-2 h-4 w-4 animate-spin" />
												处理中...
										{:else if taskControlStatus.is_paused}
											<PlayIcon class="mr-2 h-4 w-4" />
											恢复任务
										{:else}
											<PauseIcon class="mr-2 h-4 w-4" />
											停止任务
										{/if}
										</Button>

										<Button
											size="sm"
											variant="outline"
											onclick={refreshTasks}
											disabled={loadingTaskRefresh}
											class="w-full"
											title="立即刷新任务（触发新一轮扫描/下载）"
										>
											{#if loadingTaskRefresh}
												<SettingsIcon class="mr-2 h-4 w-4 animate-spin" />
												刷新中...
											{:else}
												<RefreshCwIcon class="mr-2 h-4 w-4" />
												任务刷新
											{/if}
										</Button>
									</div>
								{/if}
							</div>
						{:else}
							<div class="text-muted-foreground text-sm">加载中...</div>
						{/if}
					</CardContent>
				</Card>
			</div>

			<!-- 第三行：系统监控 -->
			<div class="grid gap-4 md:grid-cols-2">
				<!-- 内存使用情况 -->
				<Card class="overflow-hidden">
					<CardHeader class="flex flex-row items-center justify-between space-y-0 pb-2">
						<CardTitle class="text-sm font-medium">内存使用情况</CardTitle>
						<MemoryStickIcon class="text-muted-foreground h-4 w-4" />
					</CardHeader>
					<CardContent>
						{#if sysInfo}
							<div class="mb-4 space-y-2">
								<div class="flex items-center justify-between text-sm">
									<span>当前内存使用</span>
									<span class="font-medium"
										>{formatBytes(sysInfo.used_memory)} / {formatBytes(sysInfo.total_memory)}</span
									>
								</div>
							</div>
						{/if}
						{#if memoryHistory.length > 0}
							<div class="h-[150px] w-full overflow-hidden">
								<Chart.Container config={memoryChartConfig} class="h-full w-full">
									<AreaChart
										data={memoryHistory}
										x="time"
										axis="x"
										series={[
											{
												key: 'used',
												label: memoryChartConfig.used.label,
												color: memoryChartConfig.used.color
											},
											{
												key: 'process',
												label: memoryChartConfig.process.label,
												color: memoryChartConfig.process.color
											}
										]}
										props={{
											area: {
												curve: curveNatural,
												line: { class: 'stroke-1' },
												'fill-opacity': 0.4
											},
											xAxis: {
												format: () => ''
											},
											yAxis: {
												format: (v: number) => formatBytes(v)
											}
										}}
									>
										{#snippet tooltip()}
											<MyChartTooltip
												labelFormatter={(v: string | number) => {
													const date = typeof v === 'string' ? new Date(v) : new Date(v);
													return date.toLocaleString('en-US', {
														hour: '2-digit',
														minute: '2-digit',
														second: '2-digit',
														hour12: true
													});
												}}
												valueFormatter={(v: string | number) => {
													const num = typeof v === 'string' ? parseFloat(v) : v;
													return formatBytes(num);
												}}
												indicator="line"
											/>
										{/snippet}
									</AreaChart>
								</Chart.Container>
							</div>
						{:else}
							<div class="text-muted-foreground flex h-[150px] items-center justify-center text-sm">
								等待数据...
							</div>
						{/if}
					</CardContent>
				</Card>

				<Card class="overflow-hidden">
					<CardHeader class="flex flex-row items-center justify-between space-y-0 pb-2">
						<CardTitle class="text-sm font-medium">CPU 使用情况</CardTitle>
						<CpuIcon class="text-muted-foreground h-4 w-4" />
					</CardHeader>
					<CardContent class="overflow-hidden">
						{#if sysInfo}
							<div class="mb-4 space-y-2">
								<div class="flex items-center justify-between text-sm">
									<span>当前 CPU 使用率</span>
									<span class="font-medium">{formatCpu(sysInfo.used_cpu)}</span>
								</div>
							</div>
						{/if}
						{#if cpuHistory.length > 0}
							<div class="h-[150px] w-full overflow-hidden">
								<Chart.Container config={cpuChartConfig} class="h-full w-full">
									<AreaChart
										data={cpuHistory}
										x="time"
										axis="x"
										series={[
											{
												key: 'used',
												label: cpuChartConfig.used.label,
												color: cpuChartConfig.used.color
											},
											{
												key: 'process',
												label: cpuChartConfig.process.label,
												color: cpuChartConfig.process.color
											}
										]}
										props={{
											area: {
												curve: curveNatural,
												line: { class: 'stroke-1' },
												'fill-opacity': 0.4
											},
											xAxis: {
												format: () => ''
											},
											yAxis: {
												format: (v: number) => `${v}%`
											}
										}}
									>
										{#snippet tooltip()}
											<MyChartTooltip
												labelFormatter={(v: string | number) => {
													const date = typeof v === 'string' ? new Date(v) : new Date(v);
													return date.toLocaleString('en-US', {
														hour: '2-digit',
														minute: '2-digit',
														second: '2-digit',
														hour12: true
													});
												}}
												valueFormatter={(v: string | number) => {
													const num = typeof v === 'string' ? parseFloat(v) : v;
													return formatCpu(num);
												}}
												indicator="line"
											/>
										{/snippet}
									</AreaChart>
								</Chart.Container>
							</div>
						{:else}
							<div class="text-muted-foreground flex h-[150px] items-center justify-center text-sm">
								等待数据...
							</div>
						{/if}
					</CardContent>
				</Card>
			</div>
		{/if}
	</div>
{/if}
