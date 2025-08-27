<script lang="ts">
	import { onMount } from 'svelte';
	import { Plus, RefreshCw, Trash2, Edit, Play, Pause, Settings, Video, Eye, Wifi, WifiOff, AlertTriangle } from '@lucide/svelte';
	import * as Card from '$lib/components/ui/card';
	import * as Button from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	// 使用Card布局代替Table
	import * as AlertDialog from '$lib/components/ui/alert-dialog/index.js';
	import Pagination from '$lib/components/pagination.svelte';
	import BreadCrumb from '$lib/components/bread-crumb.svelte';
	import Loading from '$lib/components/ui/Loading.svelte';
	import api from '$lib/api';
	import type { LiveMonitorConfig, LiveMonitorStatusResponse, QualityInfo } from '$lib/types';
	// import LiveMonitorForm from './components/LiveMonitorForm.svelte';
	import LiveRecordsDialog from './components/LiveRecordsDialog.svelte';
	import LiveRecordingConfig from './components/LiveRecordingConfig.svelte';

	// 状态管理
	let monitors: LiveMonitorConfig[] = [];
	let totalCount = 0;
	let currentPage = 1;
	let pageSize = 10;
	let loading = false;
	let statusLoading = false;
	let error = '';

	// 监控状态
	let monitorStatus: LiveMonitorStatusResponse | null = null;

	// B站质量等级选项
	let qualityOptions: QualityInfo[] = [
		{ qn: 10000, name: '原画', description: '最高画质，原始分辨率' },
		{ qn: 800, name: '4K', description: '4K超高清画质' },
		{ qn: 401, name: '蓝光杜比', description: '蓝光画质，支持杜比音效' },
		{ qn: 400, name: '蓝光', description: '蓝光画质' },
		{ qn: 250, name: '超清', description: '超清画质，通常为720p或1080p' },
		{ qn: 150, name: '高清', description: '高清画质，通常为720p' },
		{ qn: 80, name: '流畅', description: '流畅画质，通常为480p' }
	];

	// 对话框状态
	let deleteDialogOpen = false;
	let editDialogOpen = false;
	let recordsDialogOpen = false;
	let configDialogOpen = false;
	let deleteMonitorId: number | null = null;
	let editMonitor: LiveMonitorConfig | null = null;
	let recordsMonitorId: number | null = null;
	let saving = false;

	// 表单数据
	let formData = {
		upper_name: '',
		upper_id: null,
		room_id: null,
		short_room_id: null,
		path: '',
		enabled: true
	};

	// 对话框操作函数
	function openCreateDialog() {
		console.log('openCreateDialog called');
		console.log('Current editDialogOpen before:', editDialogOpen);
		editMonitor = null;
		editDialogOpen = true;
		console.log('editDialogOpen set to:', editDialogOpen);
		// 强制触发重新渲染
		setTimeout(() => {
			console.log('After timeout, editDialogOpen is:', editDialogOpen);
		}, 0);
	}

	function openEditDialog(monitor: LiveMonitorConfig) {
		console.log('openEditDialog called with monitor:', monitor);
		editMonitor = monitor;
		// 填充表单数据
		formData = {
			upper_name: monitor.upper_name,
			upper_id: monitor.upper_id,
			room_id: monitor.room_id,
			short_room_id: monitor.short_room_id,
			path: monitor.path,
			enabled: monitor.enabled
		};
		editDialogOpen = true;
		console.log('editDialogOpen set to:', editDialogOpen);
	}

	function openDeleteDialog(monitorId: number) {
		deleteMonitorId = monitorId;
		deleteDialogOpen = true;
	}

	function openRecordsDialog(monitorId: number) {
		console.log('openRecordsDialog called with monitorId:', monitorId);
		recordsMonitorId = monitorId;
		recordsDialogOpen = true;
		console.log('recordsDialogOpen set to:', recordsDialogOpen);
	}

	function openConfigDialog() {
		console.log('openConfigDialog called');
		configDialogOpen = true;
	}

	function closeConfigDialog() {
		configDialogOpen = false;
	}

	function closeEditDialog() {
		editDialogOpen = false;
		editMonitor = null;
		// 重置表单数据
		formData = {
			upper_name: '',
			upper_id: null,
			room_id: null,
			short_room_id: null,
			path: '',
			enabled: true
		};
	}

	// 表单提交处理
	async function handleSubmit() {
		if (saving) return;
		
		saving = true;
		error = '';
		
		try {
			if (editMonitor) {
				// 更新监控
				await handleUpdate({
					...editMonitor,
					...formData
				});
			} else {
				// 创建监控
				await handleCreate(formData);
			}
			closeEditDialog();
		} catch (err) {
			error = err instanceof Error ? err.message : '操作失败';
		} finally {
			saving = false;
		}
	}

	function closeRecordsDialog() {
		recordsDialogOpen = false;
		recordsMonitorId = null;
	}

	// 面包屑导航
	const breadcrumbs = [
		{ name: '首页', href: '/' },
		{ name: '直播监控', href: '/live-monitor' }
	];

	// 获取监控列表
	async function loadMonitors() {
		loading = true;
		error = '';
		try {
			const response = await api.getLiveMonitors(currentPage, pageSize);
			if (response.status_code === 200) {
				monitors = response.data.monitors;
				totalCount = response.data.total_count;
			}
		} catch (err) {
			error = err instanceof Error ? err.message : '获取监控列表失败';
		} finally {
			loading = false;
		}
	}

	// 获取监控状态
	async function loadStatus() {
		statusLoading = true;
		try {
			const response = await api.getLiveMonitorStatus();
			if (response.status_code === 200) {
				monitorStatus = response.data;
			}
		} catch (err) {
			console.error('获取监控状态失败:', err);
		} finally {
			statusLoading = false;
		}
	}

	// 删除监控
	async function handleDelete() {
		if (!deleteMonitorId) return;
		
		try {
			await api.deleteLiveMonitor(deleteMonitorId);
			await loadMonitors();
			await loadStatus();
			deleteDialogOpen = false;
			deleteMonitorId = null;
		} catch (err) {
			error = err instanceof Error ? err.message : '删除监控失败';
		}
	}

	// 更新监控
	async function handleUpdate(monitor: LiveMonitorConfig) {
		try {
			await api.updateLiveMonitor(monitor.id, {
				upper_name: monitor.upper_name,
				room_id: monitor.room_id,
				short_room_id: monitor.short_room_id,
				path: monitor.path,
				enabled: monitor.enabled
			});
			await loadMonitors();
			await loadStatus();
			editDialogOpen = false;
			editMonitor = null;
		} catch (err) {
			error = err instanceof Error ? err.message : '更新监控失败';
		}
	}

	// 创建监控
	async function handleCreate(monitor: any) {
		try {
			await api.createLiveMonitor(monitor);
			await loadMonitors();
			await loadStatus();
		} catch (err) {
			error = err instanceof Error ? err.message : '创建监控失败';
		}
	}

	// 切换启用状态
	async function toggleEnabled(monitor: LiveMonitorConfig) {
		try {
			await api.updateLiveMonitor(monitor.id, { enabled: !monitor.enabled });
			await loadMonitors();
			await loadStatus();
		} catch (err) {
			error = err instanceof Error ? err.message : '更新状态失败';
		}
	}

	// 格式化时间
	function formatDateTime(dateString?: string) {
		if (!dateString) return '从未';
		return new Date(dateString).toLocaleString('zh-CN');
	}

	// 获取状态标签
	function getStatusBadge(status: number) {
		switch (status) {
			case 0: return { text: '未开播', variant: 'secondary' as const };
			case 1: return { text: '直播中', variant: 'destructive' as const };
			case 2: return { text: '轮播中', variant: 'default' as const };
			default: return { text: '未知', variant: 'outline' as const };
		}
	}

	// 获取连接状态标签
	function getConnectionStatusBadge() {
		if (!monitorStatus?.running) {
			return {
				text: '服务未运行',
				variant: 'secondary' as const,
				showWifiOff: true
			};
		}

		const activeCount = monitorStatus.active_monitors || 0;
		const totalCount = monitorStatus.total_monitors || 0;
		
		if (totalCount === 0) {
			return {
				text: 'WebSocket 已连接（无监控）',
				variant: 'outline' as const,
				showWifi: true
			};
		}

		if (activeCount === totalCount) {
			return {
				text: `WebSocket 已连接（${activeCount}/${totalCount}）`,
				variant: 'default' as const,
				showWifi: true
			};
		}

		return {
			text: `WebSocket 部分连接（${activeCount}/${totalCount}）`,
			variant: 'outline' as const,
			showAlert: true
		};
	}

	// 获取画质文本
	function getQualityText(qualityLevel: number) {
		const quality = qualityOptions.find(q => q.qn === qualityLevel);
		return quality ? quality.name : `质量${qualityLevel}`;
	}

	// 加载B站质量等级选项
	async function loadQualityOptions() {
		try {
			const levels = await api.getLiveQualityLevels();
			if (levels && levels.length > 0) {
				qualityOptions = levels;
			}
		} catch (error) {
			console.warn('无法加载B站质量等级，使用默认选项:', error);
		}
	}

	// 分页处理
	function handlePageChange(page: number) {
		currentPage = page;
		loadMonitors();
	}

	// 页面初始化
	onMount(() => {
		console.log('Page mounted, initial editDialogOpen:', editDialogOpen);
		loadQualityOptions();
		loadMonitors();
		loadStatus();
	});
</script>

<div class="flex flex-1 flex-col space-y-6 p-6">
	<BreadCrumb {breadcrumbs} />
	
	<div class="flex items-center justify-between">
		<div>
			<h1 class="text-3xl font-bold tracking-tight">直播监控</h1>
			<p class="text-muted-foreground">WebSocket 实时监控，即时检测开播并自动录制</p>
		</div>
		<div class="flex items-center gap-3">
			<Badge variant={getConnectionStatusBadge().variant} class="gap-1">
				{#snippet children()}
					{#if getConnectionStatusBadge().showWifiOff}
						<WifiOff class="h-3 w-3" />
					{:else if getConnectionStatusBadge().showWifi}
						<Wifi class="h-3 w-3" />
					{:else if getConnectionStatusBadge().showAlert}
						<AlertTriangle class="h-3 w-3" />
					{/if}
					{getConnectionStatusBadge().text}
				{/snippet}
			</Badge>
			<Button.Root on:click={loadMonitors} disabled={loading}>
				<RefreshCw class="mr-2 h-4 w-4 {loading ? 'animate-spin' : ''}" />
				刷新
			</Button.Root>
			<button 
				class="inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-md text-sm font-medium outline-none transition-all focus-visible:ring-[3px] disabled:pointer-events-none disabled:opacity-50 border border-input bg-background shadow-xs hover:bg-accent hover:text-accent-foreground h-9 px-4 py-2"
				on:click={(e) => {
					console.log('Config button clicked, event:', e);
					e.preventDefault();
					e.stopPropagation();
					openConfigDialog();
				}}
			>
				<Settings class="mr-2 h-4 w-4" />
				录制配置
			</button>
			<button 
				class="inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-md text-sm font-medium outline-none transition-all focus-visible:ring-[3px] disabled:pointer-events-none disabled:opacity-50 bg-primary text-primary-foreground shadow-xs hover:bg-primary/90 h-9 px-4 py-2"
				on:click={(e) => {
					console.log('Direct button clicked, event:', e);
					e.preventDefault();
					e.stopPropagation();
					openCreateDialog();
				}}
			>
				<Plus class="mr-2 h-4 w-4" />
				添加监控
			</button>
		</div>
	</div>

	<!-- 监控状态概览 -->
	{#if monitorStatus}
		<div class="grid gap-4 md:grid-cols-2 lg:grid-cols-4">
			<Card.Root>
				<Card.Header class="flex flex-row items-center justify-between space-y-0 pb-2">
					<Card.Title class="text-sm font-medium">监控模式</Card.Title>
					{#if monitorStatus.running}
						<Wifi class="h-4 w-4 text-green-500" />
					{:else}
						<WifiOff class="h-4 w-4 text-muted-foreground" />
					{/if}
				</Card.Header>
				<Card.Content>
					<div class="text-2xl font-bold">
						{#if statusLoading}
							<Loading />
						{:else}
							<Badge variant={monitorStatus.running ? "default" : "destructive"}>
								{#snippet children()}
									{monitorStatus.running ? 'WebSocket 实时' : '服务停止'}
								{/snippet}
							</Badge>
						{/if}
					</div>
					<p class="text-xs text-muted-foreground mt-1">
						{monitorStatus.running ? '实时监控，无延迟检测' : '监控服务未运行'}
					</p>
				</Card.Content>
			</Card.Root>

			<Card.Root>
				<Card.Header class="flex flex-row items-center justify-between space-y-0 pb-2">
					<Card.Title class="text-sm font-medium">总监控数</Card.Title>
					<Eye class="h-4 w-4 text-muted-foreground" />
				</Card.Header>
				<Card.Content>
					<div class="text-2xl font-bold">{monitorStatus.total_monitors}</div>
				</Card.Content>
			</Card.Root>

			<Card.Root>
				<Card.Header class="flex flex-row items-center justify-between space-y-0 pb-2">
					<Card.Title class="text-sm font-medium">启用监控</Card.Title>
					<Play class="h-4 w-4 text-muted-foreground" />
				</Card.Header>
				<Card.Content>
					<div class="text-2xl font-bold">{monitorStatus.enabled_monitors}</div>
				</Card.Content>
			</Card.Root>

			<Card.Root>
				<Card.Header class="flex flex-row items-center justify-between space-y-0 pb-2">
					<Card.Title class="text-sm font-medium">正在录制</Card.Title>
					<Video class="h-4 w-4 text-muted-foreground" />
				</Card.Header>
				<Card.Content>
					<div class="text-2xl font-bold">{monitorStatus.active_recordings}</div>
				</Card.Content>
			</Card.Root>
		</div>
	{/if}

	<!-- 错误提示 -->
	{#if error}
		<div class="rounded-md bg-destructive/15 p-4 text-destructive">
			{error}
		</div>
	{/if}

	<!-- 监控列表 -->
	<Card.Root>
		<Card.Header>
			<Card.Title>监控列表</Card.Title>
			<Card.Description>共 {totalCount} 个监控配置</Card.Description>
		</Card.Header>
		<Card.Content>
			{#if loading}
				<div class="flex justify-center p-8">
					<Loading />
				</div>
			{:else if monitors.length === 0}
				<div class="text-center p-8 text-muted-foreground">
					<Video class="mx-auto h-12 w-12 mb-4" />
					<p>暂无监控配置</p>
					<button 
						class="mt-4 inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-md text-sm font-medium outline-none transition-all focus-visible:ring-[3px] disabled:pointer-events-none disabled:opacity-50 bg-primary text-primary-foreground shadow-xs hover:bg-primary/90 h-9 px-4 py-2"
						on:click={(e) => {
							console.log('Add first monitor button clicked, event:', e);
							e.preventDefault();
							e.stopPropagation();
							openCreateDialog();
						}}
					>
						<Plus class="mr-2 h-4 w-4" />
						添加第一个监控
					</button>
				</div>
			{:else}
				<div class="grid gap-4">
					{#each monitors as monitor}
						<Card.Root>
							<Card.Content class="p-6">
								<div class="flex items-center justify-between">
									<div class="flex items-center space-x-4">
										<!-- UP主信息 -->
										<div>
											<h3 class="font-medium">{monitor.upper_name}</h3>
											<p class="text-sm text-muted-foreground">ID: {monitor.upper_id}</p>
										</div>

										<!-- 直播间信息 -->
										<div class="border-l pl-4">
											<p class="text-sm">直播间: {monitor.room_id}</p>
											{#if monitor.short_room_id}
												<p class="text-sm text-muted-foreground">短号: {monitor.short_room_id}</p>
											{/if}
										</div>

										<!-- 状态标签 -->
										<div class="flex flex-col gap-2">
											<Badge variant={monitor.enabled ? "default" : "secondary"}>
												{#snippet children()}
													{monitor.enabled ? '启用' : '禁用'}
												{/snippet}
											</Badge>
											<Badge variant={getStatusBadge(monitor.last_status).variant}>
												{#snippet children()}
													{getStatusBadge(monitor.last_status).text}
												{/snippet}
											</Badge>
										</div>

										<!-- 录制设置 -->
										<div class="border-l pl-4">
											<p class="text-sm">画质: {getQualityText(monitor.quality_level)}</p>
											<p class="text-sm text-muted-foreground">格式: {monitor.format.toUpperCase()}</p>
										</div>

										<!-- 最后检查时间 -->
										<div class="border-l pl-4">
											<p class="text-sm">最后检查</p>
											<p class="text-sm text-muted-foreground">{formatDateTime(monitor.last_check_at)}</p>
										</div>
									</div>

									<!-- 操作按钮 -->
									<div class="flex items-center gap-2">
										<button
											class="inline-flex items-center justify-center whitespace-nowrap rounded-md text-sm font-medium ring-offset-background transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:pointer-events-none disabled:opacity-50 hover:bg-accent hover:text-accent-foreground h-8 gap-1.5 px-2.5"
											on:click={(e) => {
												console.log('Toggle button clicked for monitor:', monitor.id);
												e.preventDefault();
												e.stopPropagation();
												toggleEnabled(monitor);
											}}
											title={monitor.enabled ? '暂停监控' : '启用监控'}
										>
											{#if monitor.enabled}
												<Pause class="h-4 w-4" />
											{:else}
												<Play class="h-4 w-4" />
											{/if}
										</button>
										<button
											class="inline-flex items-center justify-center whitespace-nowrap rounded-md text-sm font-medium ring-offset-background transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:pointer-events-none disabled:opacity-50 hover:bg-accent hover:text-accent-foreground h-8 gap-1.5 px-2.5"
											on:click={(e) => {
												console.log('Records button clicked for monitor:', monitor.id);
												e.preventDefault();
												e.stopPropagation();
												openRecordsDialog(monitor.id);
											}}
											title="查看录制记录"
										>
											<Video class="h-4 w-4" />
										</button>
										<button
											class="inline-flex items-center justify-center whitespace-nowrap rounded-md text-sm font-medium ring-offset-background transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:pointer-events-none disabled:opacity-50 hover:bg-accent hover:text-accent-foreground h-8 gap-1.5 px-2.5"
											on:click={(e) => {
												console.log('Edit button clicked for monitor:', monitor.id);
												e.preventDefault();
												e.stopPropagation();
												openEditDialog(monitor);
											}}
											title="编辑监控"
										>
											<Edit class="h-4 w-4" />
										</button>
										<button
											class="inline-flex items-center justify-center whitespace-nowrap rounded-md text-sm font-medium ring-offset-background transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:pointer-events-none disabled:opacity-50 hover:bg-accent hover:text-accent-foreground h-8 gap-1.5 px-2.5"
											on:click={(e) => {
												console.log('Delete button clicked for monitor:', monitor.id);
												e.preventDefault();
												e.stopPropagation();
												openDeleteDialog(monitor.id);
											}}
											title="删除监控"
										>
											<Trash2 class="h-4 w-4" />
										</button>
									</div>
								</div>
							</Card.Content>
						</Card.Root>
					{/each}
				</div>

				<!-- 分页 -->
				{#if totalCount > pageSize}
					<div class="mt-6">
						<Pagination
							{currentPage}
							totalItems={totalCount}
							itemsPerPage={pageSize}
							onPageChange={handlePageChange}
						/>
					</div>
				{/if}
			{/if}
		</Card.Content>
	</Card.Root>
</div>

<!-- 编辑/创建监控对话框 -->
{#if editDialogOpen}
	<div class="fixed inset-0 z-[9999] bg-black/80" role="presentation">
		<div class="fixed left-[50%] top-[50%] z-[10000] grid w-full max-w-2xl translate-x-[-50%] translate-y-[-50%] gap-4 border bg-background p-6 shadow-lg duration-200 rounded-lg" on:click|stopPropagation on:keydown={(e) => e.key === 'Escape' && closeEditDialog()} role="dialog" tabindex="-1">
			<div class="flex flex-col space-y-1.5 text-center sm:text-left">
				<h2 class="text-lg font-semibold leading-none tracking-tight">
					{editMonitor ? '编辑监控' : '创建监控'}
				</h2>
				<p class="text-sm text-muted-foreground">
					{editMonitor ? '修改直播监控配置' : '添加新的直播监控配置'}
				</p>
			</div>
			<form class="space-y-4" on:submit|preventDefault={handleSubmit}>
				<div class="grid grid-cols-1 md:grid-cols-2 gap-4">
					<!-- UP主信息 -->
					<div>
						<label for="upper-name" class="block text-sm font-medium mb-2">UP主名称</label>
						<input 
							id="upper-name"
							type="text" 
							bind:value={formData.upper_name}
							class="w-full px-3 py-2 border border-input bg-background rounded-md focus:outline-none focus:ring-2 focus:ring-ring"
							placeholder="请输入UP主名称"
							required
						/>
					</div>
					
					<div>
						<label for="upper-id" class="block text-sm font-medium mb-2">UP主ID</label>
						<input 
							id="upper-id"
							type="number" 
							bind:value={formData.upper_id}
							class="w-full px-3 py-2 border border-input bg-background rounded-md focus:outline-none focus:ring-2 focus:ring-ring"
							placeholder="请输入UP主ID"
						/>
					</div>
				</div>

				<div class="grid grid-cols-1 md:grid-cols-2 gap-4">
					<!-- 直播间信息 -->
					<div>
						<label for="room-id" class="block text-sm font-medium mb-2">直播间ID</label>
						<input 
							id="room-id"
							type="number" 
							bind:value={formData.room_id}
							class="w-full px-3 py-2 border border-input bg-background rounded-md focus:outline-none focus:ring-2 focus:ring-ring"
							placeholder="请输入直播间ID"
							required
						/>
					</div>
					
					<div>
						<label for="short-room-id" class="block text-sm font-medium mb-2">直播间短号</label>
						<input 
							id="short-room-id"
							type="number" 
							bind:value={formData.short_room_id}
							class="w-full px-3 py-2 border border-input bg-background rounded-md focus:outline-none focus:ring-2 focus:ring-ring"
							placeholder="直播间短号（可选）"
						/>
					</div>
				</div>

				<!-- 录制设置 -->
				<div>
					<label for="save-path" class="block text-sm font-medium mb-2">保存路径</label>
					<input 
						id="save-path"
						type="text" 
						bind:value={formData.path}
						class="w-full px-3 py-2 border border-input bg-background rounded-md focus:outline-none focus:ring-2 focus:ring-ring"
						placeholder="录制文件保存路径"
						required
					/>
				</div>

				<div class="text-sm text-muted-foreground mb-4 p-4 bg-muted/50 rounded-lg">
					<p class="font-medium mb-2">🎥 录制配置说明</p>
					<p>录制画质和格式将使用全局配置中的设置。请在「直播录制配置」中设置画质和格式选项。</p>
				</div>

				<div class="flex items-center space-x-2">
					<input 
						type="checkbox" 
						bind:checked={formData.enabled}
						class="w-4 h-4"
						id="enabled"
					/>
					<label for="enabled" class="text-sm font-medium">启用监控</label>
				</div>

				<!-- 按钮组 -->
				<div class="flex justify-end gap-3 pt-4 border-t">
					<button 
						type="button"
						on:click={closeEditDialog}
						class="px-4 py-2 text-sm font-medium rounded-md border border-input bg-background hover:bg-accent hover:text-accent-foreground"
					>
						取消
					</button>
					<button 
						type="submit"
						disabled={saving}
						class="px-4 py-2 text-sm font-medium rounded-md bg-primary text-primary-foreground hover:bg-primary/90 disabled:opacity-50"
					>
						{saving ? '保存中...' : (editMonitor ? '更新监控' : '创建监控')}
					</button>
				</div>
			</form>
		</div>
	</div>
{/if}

<!-- 删除确认对话框 -->
<AlertDialog.Root bind:open={deleteDialogOpen}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>确认删除</AlertDialog.Title>
			<AlertDialog.Description>
				此操作将永久删除该监控配置，且无法恢复。确定要继续吗？
			</AlertDialog.Description>
		</AlertDialog.Header>
		<AlertDialog.Footer>
			<button 
				class="inline-flex items-center justify-center whitespace-nowrap rounded-md text-sm font-medium ring-offset-background transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:pointer-events-none disabled:opacity-50 border border-input bg-background hover:bg-accent hover:text-accent-foreground h-10 px-4 py-2"
				on:click={(e) => {
					console.log('Cancel button clicked');
					e.preventDefault();
					e.stopPropagation();
					deleteDialogOpen = false;
					deleteMonitorId = null;
				}}
			>
				取消
			</button>
			<button 
				class="inline-flex items-center justify-center whitespace-nowrap rounded-md text-sm font-medium ring-offset-background transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:pointer-events-none disabled:opacity-50 bg-destructive text-destructive-foreground hover:bg-destructive/90 h-10 px-4 py-2"
				on:click={(e) => {
					console.log('Delete confirmation button clicked');
					e.preventDefault();
					e.stopPropagation();
					handleDelete();
				}}
			>
				删除
			</button>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>

<!-- 录制记录对话框 -->
{#if recordsDialogOpen && recordsMonitorId}
	<LiveRecordsDialog 
		monitorId={recordsMonitorId}
		open={recordsDialogOpen}
		onClose={closeRecordsDialog}
	/>
{/if}

<!-- 录制配置对话框 -->
{#if configDialogOpen}
	<LiveRecordingConfig 
		on:close={closeConfigDialog}
	/>
{/if}