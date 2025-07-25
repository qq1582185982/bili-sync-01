<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import { toast } from 'svelte-sonner';
	import { Button } from '$lib/components/ui/button';
	// import * as Tabs from '$lib/components/ui/tabs'; // 未使用，已注释
	import * as Card from '$lib/components/ui/card';
	import { Badge } from '$lib/components/ui/badge';
	import { setBreadcrumb } from '$lib/stores/breadcrumb';
	import api from '$lib/api';
	import { RefreshCw, Download, AlertTriangle, XCircle, Info, Bug } from '@lucide/svelte';

	// 日志级别类型
	type LogLevel = 'info' | 'warn' | 'error' | 'debug';

	// 日志条目类型
	interface LogEntry {
		timestamp: string;
		level: LogLevel;
		message: string;
		target?: string;
	}

	// 日志响应类型
	// interface LogsResponse { // 未使用，已注释
	// 	logs: LogEntry[];
	// 	total: number;
	// }

	// 响应式变量
	let innerWidth = 0;
	$: isMobile = innerWidth < 768;

	// 状态变量
	let logs: LogEntry[] = [];
	let filteredLogs: LogEntry[] = [];
	let isLoading = false;
	let autoRefresh = true;
	let refreshInterval: number;
	let currentTab = 'all';
	let isAuthenticated = false;
	let authError = '';
	// let logLimit = 500; // 可自定义的日志数量限制 - 未使用，已注释
	let totalLogCount = 0; // 总日志数量

	// 分页相关变量
	let currentPage = 1;
	let totalPages = 0;
	let perPage = 100;

	// 日志级别颜色映射
	const levelColors: Record<LogLevel, string> = {
		info: 'bg-blue-100 text-blue-800 dark:bg-blue-900 dark:text-blue-200',
		warn: 'bg-yellow-100 text-yellow-800 dark:bg-yellow-900 dark:text-yellow-200',
		error: 'bg-red-100 text-red-800 dark:bg-red-900 dark:text-red-200',
		debug: 'bg-gray-100 text-gray-800 dark:bg-gray-900 dark:text-gray-200'
	};

	// 日志级别图标映射
	// eslint-disable-next-line @typescript-eslint/no-explicit-any
	const levelIcons: Record<LogLevel, any> = {
		info: Info,
		warn: AlertTriangle,
		error: XCircle,
		debug: Bug
	};

	// 检查认证状态
	async function checkAuth(): Promise<boolean> {
		const token = localStorage.getItem('auth_token');
		if (!token) {
			authError = '未找到认证token，请先登录';
			return false;
		}

		try {
			// 尝试调用一个需要认证的API来验证Token
			await api.getVideoSources();
			return true;
		} catch (error: unknown) {
			console.error('认证验证失败:', error);
			authError = '认证失败，请重新登录';
			return false;
		}
	}

	// 初始化面包屑
	onMount(async () => {
		setBreadcrumb([
			{ label: '首页', href: '/' },
			{ label: '系统日志', href: '/logs' }
		]);

		// 验证认证状态
		isAuthenticated = await checkAuth();

		if (isAuthenticated) {
			// 加载日志
			await loadLogs();

			// 设置自动刷新
			if (autoRefresh) {
				refreshInterval = setInterval(() => handleRefresh(), 5000); // 每5秒刷新一次
			}
		}
	});

	onDestroy(() => {
		if (refreshInterval) {
			clearInterval(refreshInterval);
		}
	});

	// 加载日志
	async function loadLogs(level?: LogLevel, page: number = currentPage) {
		if (!isAuthenticated) {
			return;
		}

		try {
			isLoading = true;
			authError = '';
			const params = new URLSearchParams();
			params.append('limit', perPage.toString());
			params.append('page', page.toString());

			if (level) {
				params.append('level', level);
			}

			const token = localStorage.getItem('auth_token');
			if (!token) {
				throw new Error('未找到认证token');
			}

			// 使用fetch直接调用API
			const response = await fetch(`/api/logs?${params.toString()}`, {
				headers: {
					Authorization: token,
					'Content-Type': 'application/json'
				}
			});

			if (!response.ok) {
				if (response.status === 401) {
					isAuthenticated = false;
					authError = '认证失败，请重新登录';
					return;
				}
				throw new Error(`HTTP ${response.status}: 加载日志失败`);
			}

			const result = await response.json();
			console.log('API响应:', result);

			// 修复数据解析逻辑 - 处理所有可能的响应格式
			let logsArray: LogEntry[] = [];
			// eslint-disable-next-line @typescript-eslint/no-explicit-any
			let responseData: any = {};

			if (result.status_code === 200 && result.data) {
				// 新格式：{status_code: 200, data: {logs: [...], total: ...}}
				responseData = result.data;
				if (result.data.logs && Array.isArray(result.data.logs)) {
					logsArray = result.data.logs;
					console.log('从status_code格式获取数据，长度:', logsArray.length);
				}
			} else if (result.success && result.data) {
				// 标准包装格式 {success: true, data: {logs: [...], total: ...}}
				responseData = result.data;
				if (result.data.logs && Array.isArray(result.data.logs)) {
					logsArray = result.data.logs;
					console.log('从success格式获取数据，长度:', logsArray.length);
				}
			} else if (result.logs && Array.isArray(result.logs)) {
				// 直接格式 {logs: [...], total: ...}
				responseData = result;
				logsArray = result.logs;
				console.log('从直接格式获取数据，长度:', logsArray.length);
			} else if (Array.isArray(result)) {
				// 纯数组格式 [...]
				logsArray = result;
				responseData = {
					logs: result,
					total: result.length,
					page: 1,
					per_page: result.length,
					total_pages: 1
				};
				console.log('从数组格式获取数据，长度:', logsArray.length);
			} else {
				console.warn('无法识别的API响应格式:', result);
				console.log('响应键:', Object.keys(result));
			}

			// 更新分页信息
			logs = logsArray;
			totalLogCount = responseData.total || logsArray.length;
			currentPage = responseData.page || page;
			totalPages = responseData.total_pages || 1;
			perPage = responseData.per_page || perPage;

			console.log('分页信息:', { currentPage, totalPages, perPage, total: totalLogCount });
			console.log('最终设置的logs长度:', logs.length);
			if (logs.length > 0) {
				console.log('前几条日志:', logs.slice(0, 2));
			}

			filterLogs();
		} catch (error: unknown) {
			console.error('加载日志失败:', error);
			const errorMessage = error instanceof Error ? error.message : '加载日志失败';
			authError = errorMessage;
			toast.error('加载日志失败', {
				description: errorMessage
			});
		} finally {
			isLoading = false;
		}
	}

	// 根据当前选项卡过滤日志
	function filterLogs() {
		// 注意：现在我们使用服务器端过滤，这里主要用于显示
		// 实际的过滤在loadLogs函数中通过API参数完成
		filteredLogs = logs;
	}

	// 分页相关函数
	function goToPage(page: number) {
		if (page >= 1 && page <= totalPages && page !== currentPage) {
			const level = currentTab === 'all' ? undefined : (currentTab as LogLevel);
			loadLogs(level, page);
		}
	}

	function goToFirstPage() {
		goToPage(1);
	}

	function goToLastPage() {
		goToPage(totalPages);
	}

	function goToPrevPage() {
		goToPage(currentPage - 1);
	}

	function goToNextPage() {
		goToPage(currentPage + 1);
	}

	// 手动刷新
	async function handleRefresh() {
		const level = currentTab === 'all' ? undefined : (currentTab as LogLevel);
		await loadLogs(level, currentPage);
	}

	// 切换自动刷新
	function toggleAutoRefresh() {
		autoRefresh = !autoRefresh;
		if (autoRefresh) {
			refreshInterval = setInterval(() => handleRefresh(), 5000);
		} else {
			clearInterval(refreshInterval);
		}
	}

	// 导出日志
	async function exportLogs() {
		if (!isAuthenticated) return;

		try {
			isLoading = true;

			// 获取当前选择级别的所有日志
			const params = new URLSearchParams();
			// 使用较大的limit值来获取尽可能多的日志
			params.append('limit', '50000');
			params.append('page', '1');

			if (currentTab !== 'all') {
				params.append('level', currentTab);
			}

			const token = localStorage.getItem('auth_token');
			if (!token) {
				throw new Error('未找到认证token');
			}

			const response = await fetch(`/api/logs?${params.toString()}`, {
				headers: {
					Authorization: token,
					'Content-Type': 'application/json'
				}
			});

			if (!response.ok) {
				throw new Error(`HTTP ${response.status}: 获取日志失败`);
			}

			const result = await response.json();

			// 解析响应数据
			let allLogs: LogEntry[] = [];
			if (result.status_code === 200 && result.data) {
				allLogs = result.data.logs || [];
			} else if (result.success && result.data) {
				allLogs = result.data.logs || [];
			} else if (result.logs && Array.isArray(result.logs)) {
				allLogs = result.logs;
			} else if (Array.isArray(result)) {
				allLogs = result;
			}

			if (allLogs.length === 0) {
				toast.error('没有日志可导出');
				return;
			}

			// 生成CSV内容
			const csvContent = [
				'时间,级别,消息,来源',
				...allLogs.map(
					(log) =>
						`"${formatTimestamp(log.timestamp)}","${log.level}","${log.message.replace(/"/g, '""')}","${log.target || ''}"`
				)
			].join('\n');

			// 创建文件名
			const levelText = currentTab === 'all' ? '全部' : currentTab;
			const fileName = `logs-${levelText}-${new Date().toISOString().split('T')[0]}.csv`;

			// 下载文件
			const blob = new Blob([csvContent], { type: 'text/csv;charset=utf-8;' });
			const link = document.createElement('a');
			link.href = URL.createObjectURL(blob);
			link.download = fileName;
			link.click();

			toast.success(`成功导出 ${allLogs.length} 条${levelText}日志`);
		} catch (error: unknown) {
			console.error('导出日志失败:', error);
			toast.error('导出日志失败', {
				description: error instanceof Error ? error.message : '未知错误'
			});
		} finally {
			isLoading = false;
		}
	}

	// 格式化时间戳
	function formatTimestamp(timestamp: string): string {
		return new Date(timestamp).toLocaleString('zh-CN', {
			year: 'numeric',
			month: '2-digit',
			day: '2-digit',
			hour: '2-digit',
			minute: '2-digit',
			second: '2-digit'
		});
	}

	// 重新登录
	function goToLogin() {
		window.location.href = '/';
	}
</script>

<svelte:window bind:innerWidth />

{#if !isAuthenticated}
	<!-- 未认证状态 -->
	<div class="container mx-auto py-12">
		<div class="text-center">
			<h1 class="mb-4 text-3xl font-bold">访问被拒绝</h1>
			<p class="text-muted-foreground mb-6">{authError}</p>
			<Button onclick={goToLogin}>返回登录</Button>
		</div>
	</div>
{:else}
	<!-- 已认证状态 - 显示日志界面 -->
	<div class="container mx-auto space-y-6">
		<!-- 页面标题和操作按钮 -->
		<div class="flex {isMobile ? 'flex-col gap-4' : 'items-center justify-between'}">
			<div>
				<h1 class="text-3xl font-bold tracking-tight">系统日志</h1>
				<p class="text-muted-foreground">查看系统运行日志和错误信息</p>
				{#if authError}
					<p class="mt-1 text-sm text-red-600">{authError}</p>
				{/if}
			</div>

			<div class="flex {isMobile ? 'flex-col' : ''} gap-2">
				<!-- 日志数量选择 -->
				<div class="flex items-center gap-2">
					<label for="perPage" class="text-sm font-medium whitespace-nowrap">每页显示:</label>
					<select
						id="perPage"
						bind:value={perPage}
						on:change={() => {
							currentPage = 1;
							const level = currentTab === 'all' ? undefined : (currentTab as LogLevel);
							loadLogs(level, 1);
						}}
						class="border-input bg-background h-8 rounded-md border px-2 py-1 text-sm"
					>
						<option value={50}>50</option>
						<option value={100}>100</option>
						<option value={200}>200</option>
						<option value={500}>500</option>
						<option value={1000}>1000</option>
						<option value={5000}>5000</option>
					</select>
				</div>

				<Button variant="outline" size="sm" onclick={handleRefresh} disabled={isLoading}>
					<RefreshCw class="h-4 w-4 {isLoading ? 'animate-spin' : ''}" />
					刷新
				</Button>

				<Button
					variant="outline"
					size="sm"
					onclick={toggleAutoRefresh}
					class={autoRefresh
						? 'border-green-200 bg-green-50 text-green-700 hover:bg-green-100'
						: ''}
				>
					{autoRefresh ? '自动刷新中' : '开启自动刷新'}
				</Button>

				<Button
					variant="outline"
					size="sm"
					onclick={exportLogs}
					disabled={isLoading || !isAuthenticated}
				>
					<Download class="h-4 w-4" />
					导出{currentTab === 'all' ? '全部' : currentTab}日志
				</Button>
			</div>
		</div>

		<!-- 日志选项卡 -->
		<div class="space-y-4">
			<!-- 选项卡按钮 -->
			<div class="bg-muted flex space-x-1 rounded-lg p-1">
				<button
					class="flex-1 rounded-md px-3 py-2 text-sm font-medium transition-colors {currentTab ===
					'all'
						? 'bg-background text-foreground shadow-sm'
						: 'text-muted-foreground hover:text-foreground'}"
					on:click={() => {
						currentTab = 'all';
						currentPage = 1;
						loadLogs();
					}}
				>
					全部日志
				</button>
				<button
					class="flex-1 rounded-md px-3 py-2 text-sm font-medium transition-colors {currentTab ===
					'info'
						? 'bg-background text-foreground shadow-sm'
						: 'text-muted-foreground hover:text-foreground'}"
					on:click={() => {
						currentTab = 'info';
						currentPage = 1;
						loadLogs('info', 1);
					}}
				>
					信息
				</button>
				<button
					class="flex-1 rounded-md px-3 py-2 text-sm font-medium transition-colors {currentTab ===
					'warn'
						? 'bg-background text-foreground shadow-sm'
						: 'text-muted-foreground hover:text-foreground'}"
					on:click={() => {
						currentTab = 'warn';
						currentPage = 1;
						loadLogs('warn', 1);
					}}
				>
					警告
				</button>
				<button
					class="flex-1 rounded-md px-3 py-2 text-sm font-medium transition-colors {currentTab ===
					'error'
						? 'bg-background text-foreground shadow-sm'
						: 'text-muted-foreground hover:text-foreground'}"
					on:click={() => {
						currentTab = 'error';
						currentPage = 1;
						loadLogs('error', 1);
					}}
				>
					错误
				</button>
				<button
					class="flex-1 rounded-md px-3 py-2 text-sm font-medium transition-colors {currentTab ===
					'debug'
						? 'bg-background text-foreground shadow-sm'
						: 'text-muted-foreground hover:text-foreground'}"
					on:click={() => {
						currentTab = 'debug';
						currentPage = 1;
						loadLogs('debug', 1);
					}}
				>
					调试
				</button>
			</div>

			<!-- 日志内容 -->
			<Card.Root>
				<Card.Header>
					<Card.Title class="flex items-center gap-2">
						日志列表
						{#if isLoading}
							<RefreshCw class="h-4 w-4 animate-spin" />
						{/if}
					</Card.Title>
					<Card.Description>
						显示 {filteredLogs.length} 条日志
						{#if totalLogCount > logs.length}
							(共 {totalLogCount} 条，当前加载 {logs.length} 条)
						{:else if totalLogCount > 0}
							(共 {totalLogCount} 条)
						{/if}
						{#if totalPages > 1}
							- 第 {currentPage} 页，共 {totalPages} 页
						{/if}
					</Card.Description>
				</Card.Header>
				<Card.Content class="p-0">
					<div class="h-[600px] overflow-auto">
						{#if filteredLogs.length === 0}
							<div class="text-muted-foreground flex h-32 items-center justify-center">
								{isLoading ? '加载中...' : '暂无日志'}
							</div>
						{:else}
							<div class="space-y-1 p-4">
								{#each filteredLogs as log, index (index)}
									<div class="mb-3 border-b border-gray-100 pb-3 last:border-b-0">
										<div class="flex {isMobile ? 'flex-col gap-2' : 'items-start justify-between'}">
											<div class="flex flex-1 items-start gap-3">
												<svelte:component
													this={levelIcons[log.level]}
													class="mt-1 h-4 w-4 flex-shrink-0 text-{log.level === 'error'
														? 'red'
														: log.level === 'warn'
															? 'yellow'
															: log.level === 'info'
																? 'blue'
																: 'gray'}-500"
												/>
												<div class="min-w-0 flex-1">
													<div
														class="flex {isMobile ? 'flex-col gap-1' : 'items-center gap-2'} mb-1"
													>
														<Badge variant="outline" class={levelColors[log.level]}>
															{log.level.toUpperCase()}
														</Badge>
														{#if log.target}
															<span class="text-muted-foreground font-mono text-xs"
																>{log.target}</span
															>
														{/if}
													</div>
													<p class="text-sm break-words">{log.message}</p>
												</div>
											</div>
											<div
												class="text-muted-foreground font-mono text-xs {isMobile
													? 'text-left'
													: 'text-right'} flex-shrink-0"
											>
												{formatTimestamp(log.timestamp)}
											</div>
										</div>
									</div>
								{/each}
							</div>
						{/if}
					</div>

					<!-- 分页控件 -->
					{#if totalPages > 1}
						<div class="flex items-center justify-between border-t px-4 py-3">
							<div class="text-muted-foreground text-sm">
								显示第 {(currentPage - 1) * perPage + 1} - {Math.min(
									currentPage * perPage,
									totalLogCount
								)} 条，共 {totalLogCount} 条
							</div>
							<div class="flex items-center space-x-2">
								<Button
									variant="outline"
									size="sm"
									onclick={goToFirstPage}
									disabled={currentPage === 1 || isLoading}
								>
									首页
								</Button>
								<Button
									variant="outline"
									size="sm"
									onclick={goToPrevPage}
									disabled={currentPage === 1 || isLoading}
								>
									上一页
								</Button>

								<!-- 页码按钮 -->
								{#each Array.from({ length: Math.min(5, totalPages) }, (_, i) => {
									const startPage = Math.max(1, Math.min(currentPage - 2, totalPages - 4));
									return startPage + i;
								}) as pageNum (pageNum)}
									<Button
										variant={pageNum === currentPage ? 'default' : 'outline'}
										size="sm"
										onclick={() => goToPage(pageNum)}
										disabled={isLoading}
									>
										{pageNum}
									</Button>
								{/each}

								<Button
									variant="outline"
									size="sm"
									onclick={goToNextPage}
									disabled={currentPage === totalPages || isLoading}
								>
									下一页
								</Button>
								<Button
									variant="outline"
									size="sm"
									onclick={goToLastPage}
									disabled={currentPage === totalPages || isLoading}
								>
									末页
								</Button>
							</div>
						</div>
					{/if}
				</Card.Content>
			</Card.Root>
		</div>
	</div>
{/if}
