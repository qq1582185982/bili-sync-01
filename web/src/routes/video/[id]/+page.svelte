<script lang="ts">
	import { goto } from '$app/navigation';
	import { page } from '$app/stores';
	import api from '$lib/api';
	import StatusEditor from '$lib/components/status-editor.svelte';
	import { Button } from '$lib/components/ui/button/index.js';
	import VideoCard from '$lib/components/video-card.svelte';
	import { setBreadcrumb } from '$lib/stores/breadcrumb';
	import { appStateStore, ToQuery } from '$lib/stores/filter';
	import type { ApiError, UpdateVideoStatusRequest, VideoResponse } from '$lib/types';
	import EditIcon from '@lucide/svelte/icons/edit';
	import PlayIcon from '@lucide/svelte/icons/play';
	import TrashIcon from '@lucide/svelte/icons/trash-2';
	import XIcon from '@lucide/svelte/icons/x';
	import { onMount } from 'svelte';
	import { toast } from 'svelte-sonner';

	let videoData: VideoResponse | null = null;
	let loading = false;
	let error: string | null = null;
	let resetDialogOpen = false;
	let resetting = false;
	let statusEditorOpen = false;
	let statusEditorLoading = false;
	let showVideoPlayer = false;
	let currentPlayingPageIndex = 0;
	let onlinePlayMode = false; // false: 本地播放, true: 在线播放
	let onlinePlayInfo: any = null;
	let loadingPlayInfo = false;
	let isFullscreen = false; // 是否全屏模式
	let deleteDialogOpen = false;
	let deleting = false;

	// 根据视频类型动态生成任务名称
	$: videoTaskNames = (() => {
		if (!videoData?.video) return ['视频封面', '视频信息', 'UP主头像', 'UP主信息', '分P下载'];
		
		const isBangumi = videoData.video.bangumi_title !== undefined;
		if (isBangumi) {
			// 番剧任务名称：VideoStatus[2] 对应 tvshow.nfo 生成
			return ['视频封面', '视频信息', 'tvshow.nfo', 'UP主信息', '分P下载'];
		} else {
			// 普通视频任务名称：VideoStatus[2] 对应 UP主头像下载
			return ['视频封面', '视频信息', 'UP主头像', 'UP主信息', '分P下载'];
		}
	})();

	// 检查视频是否可播放（分P下载任务已完成）
	function isVideoPlayable(video: any): boolean {
		if (video && video.download_status && Array.isArray(video.download_status)) {
			// 检查第5个任务（分P下载，索引4）是否完成（状态为7）
			return video.download_status[4] === 7;
		}
		return false;
	}

	// 获取播放的视频ID（分页ID或视频ID）
	function getPlayVideoId(): number {
		if (videoData && videoData.pages && videoData.pages.length > 0) {
			// 如果有分页，使用分页ID
			return videoData.pages[currentPlayingPageIndex].id;
		} else if (videoData) {
			// 如果没有分页（单P视频），使用视频ID
			return videoData.video.id;
		}
		return 0;
	}

	async function loadVideoDetail() {
		const videoId = parseInt($page.params.id);
		if (isNaN(videoId)) {
			error = '无效的视频ID';
			toast.error('无效的视频ID');
			return;
		}

		loading = true;
		error = null;

		try {
			const result = await api.getVideo(videoId);
			videoData = result.data;
		} catch (error) {
			console.error('加载视频详情失败:', error);
			toast.error('加载视频详情失败', {
				description: (error as ApiError).message
			});
		} finally {
			loading = false;
		}
	}

	onMount(() => {
		setBreadcrumb([
			{
				label: '主页',
				onClick: () => {
					goto(`/${ToQuery($appStateStore)}`);
				}
			},
			{ label: '视频详情', isActive: true }
		]);
	});

	// 监听路由参数变化
	$: if ($page.params.id) {
		loadVideoDetail();
	}

	async function handleStatusEditorSubmit(request: UpdateVideoStatusRequest) {
		if (!videoData) return;

		statusEditorLoading = true;
		try {
			const result = await api.updateVideoStatus(videoData.video.id, request);
			const data = result.data;

			if (data.success) {
				// 更新本地数据
				videoData = {
					video: data.video,
					pages: data.pages
				};
				statusEditorOpen = false;
				toast.success('状态更新成功');
			} else {
				toast.error('状态更新失败');
			}
		} catch (error) {
			console.error('状态更新失败:', error);
			toast.error('状态更新失败', {
				description: (error as ApiError).message
			});
		} finally {
			statusEditorLoading = false;
		}
	}

	// 获取在线播放信息
	async function loadOnlinePlayInfo(videoId: string | number) {
		if (loadingPlayInfo) return;

		loadingPlayInfo = true;
		try {
			const result = await api.getVideoPlayInfo(videoId);
			onlinePlayInfo = result.data;
			console.log('在线播放信息:', onlinePlayInfo);
		} catch (error) {
			console.error('获取播放信息失败:', error);
			toast.error('获取在线播放信息失败', {
				description: (error as ApiError).message
			});
			onlinePlayInfo = null;
		} finally {
			loadingPlayInfo = false;
		}
	}

	// 切换播放模式
	function togglePlayMode() {
		onlinePlayMode = !onlinePlayMode;
		if (onlinePlayMode && !onlinePlayInfo) {
			const videoId = getPlayVideoId();
			loadOnlinePlayInfo(videoId);
		}
	}

	// 获取视频播放源
	function getVideoSource() {
		if (onlinePlayMode && onlinePlayInfo) {
			// 在线播放模式：使用代理的B站视频流
			if (onlinePlayInfo.video_streams && onlinePlayInfo.video_streams.length > 0) {
				const videoStream = onlinePlayInfo.video_streams[0];
				return api.getProxyStreamUrl(videoStream.url);
			}
		} else {
			// 本地播放模式：使用现有的本地文件流
			return `/api/videos/stream/${getPlayVideoId()}`;
		}
		return '';
	}

	// 获取音频播放源
	function getAudioSource() {
		if (
			onlinePlayMode &&
			onlinePlayInfo &&
			onlinePlayInfo.audio_streams &&
			onlinePlayInfo.audio_streams.length > 0
		) {
			const audioStream = onlinePlayInfo.audio_streams[0];
			return api.getProxyStreamUrl(audioStream.url);
		}
		return '';
	}

	// 检查是否是DASH分离流
	function isDashSeparatedStream() {
		return (
			onlinePlayMode &&
			onlinePlayInfo &&
			onlinePlayInfo.audio_streams &&
			onlinePlayInfo.audio_streams.length > 0 &&
			onlinePlayInfo.video_streams &&
			onlinePlayInfo.video_streams.length > 0
		);
	}

	// 初始化音频同步
	function initAudioSync() {
		if (isDashSeparatedStream()) {
			setTimeout(() => {
				const audio = document.querySelector('#sync-audio') as HTMLAudioElement;
				if (audio) {
					audio.volume = 1.0; // 固定100%音量
					audio.muted = false;
				}
			}, 100);
		}
	}

	// 监听全屏变化事件
	function handleFullscreenChange() {
		isFullscreen = !!(
			document.fullscreenElement ||
			(document as any).webkitFullscreenElement ||
			(document as any).mozFullScreenElement
		);
	}

	// 组件挂载时添加全屏事件监听
	onMount(() => {
		document.addEventListener('fullscreenchange', handleFullscreenChange);
		document.addEventListener('webkitfullscreenchange', handleFullscreenChange);
		document.addEventListener('mozfullscreenchange', handleFullscreenChange);

		return () => {
			document.removeEventListener('fullscreenchange', handleFullscreenChange);
			document.removeEventListener('webkitfullscreenchange', handleFullscreenChange);
			document.removeEventListener('mozfullscreenchange', handleFullscreenChange);
		};
	});

	// 删除视频
	async function handleDeleteVideo() {
		if (!videoData) return;

		deleting = true;
		try {
			const result = await api.deleteVideo(videoData.video.id);
			const data = result.data;

			if (data.success) {
				toast.success('视频删除成功', {
					description: '视频已被标记为删除状态'
				});
				deleteDialogOpen = false;
				// 返回首页
				goto('/');
			} else {
				toast.error('视频删除失败', {
					description: data.message
				});
			}
		} catch (error) {
			console.error('删除视频失败:', error);
			toast.error('删除视频失败', {
				description: (error as ApiError).message
			});
		} finally {
			deleting = false;
		}
	}
</script>

<svelte:head>
	<title>{videoData?.video.name || '视频详情'} - Bili Sync</title>
</svelte:head>

{#if loading}
	<div class="flex items-center justify-center py-12">
		<div class="text-muted-foreground">加载中...</div>
	</div>
{:else if error}
	<div class="flex items-center justify-center py-12">
		<div class="space-y-2 text-center">
			<p class="text-destructive">{error}</p>
			<button
				class="text-muted-foreground hover:text-foreground text-sm transition-colors"
				onclick={() => goto('/')}
			>
				返回首页
			</button>
		</div>
	</div>
{:else if videoData}
	<!-- 视频信息区域 -->
	<section>
		<div class="mb-4 flex items-center justify-between">
			<h2 class="text-xl font-semibold">视频信息</h2>
			<div class="flex gap-2">
				{#if isVideoPlayable(videoData.video)}
					<Button
						size="sm"
						variant="default"
						class="shrink-0 cursor-pointer"
						onclick={() => (showVideoPlayer = true)}
					>
						<PlayIcon class="mr-2 h-4 w-4" />
						本地播放
					</Button>
				{/if}
				<Button
					size="sm"
					variant="outline"
					class="shrink-0 cursor-pointer"
					onclick={() => {
						onlinePlayMode = true;
						showVideoPlayer = true;
						if (!onlinePlayInfo) {
							const videoId = getPlayVideoId();
							loadOnlinePlayInfo(videoId);
						}
					}}
					disabled={loadingPlayInfo}
				>
					<PlayIcon class="mr-2 h-4 w-4" />
					{loadingPlayInfo ? '加载中...' : '在线播放'}
				</Button>
				<Button
					size="sm"
					variant="outline"
					class="shrink-0 cursor-pointer"
					onclick={() => (statusEditorOpen = true)}
					disabled={statusEditorLoading}
				>
					<EditIcon class="mr-2 h-4 w-4" />
					编辑状态
				</Button>
				<Button
					size="sm"
					variant="destructive"
					class="shrink-0 cursor-pointer"
					onclick={() => (deleteDialogOpen = true)}
					disabled={deleting}
				>
					<TrashIcon class="mr-2 h-4 w-4" />
					删除视频
				</Button>
			</div>
		</div>

		<div style="margin-bottom: 1rem;">
			<VideoCard
				video={{
					id: videoData.video.id,
					name: videoData.video.name,
					upper_name: videoData.video.upper_name,
					path: videoData.video.path,
					category: videoData.video.category,
					cover: videoData.video.cover || '',
					download_status: videoData.video.download_status,
					bangumi_title: videoData.video.bangumi_title
				}}
				mode="detail"
				showActions={true}
				progressHeight="h-3"
				gap="gap-2"
				taskNames={videoTaskNames}
			/>
		</div>

		<!-- 下载路径信息 -->
		{#if videoData.pages && videoData.pages.length > 0 && videoData.pages[0].path}
			<div class="mb-4 rounded-lg border bg-gray-50 p-4">
				<h3 class="mb-2 text-sm font-medium text-gray-700">📁 下载保存路径</h3>
				<div class="rounded border bg-white px-3 py-2 font-mono text-sm break-all">
					{videoData.pages[0].path}
				</div>
				<p class="mt-1 text-xs text-gray-500">视频文件将保存到此路径下</p>
			</div>
		{/if}
	</section>

	<section>
		{#if videoData.pages && videoData.pages.length > 0}
			<div class="mb-4 flex items-center justify-between">
				<h2 class="text-xl font-semibold">分页列表</h2>
				<div class="text-muted-foreground text-sm">
					共 {videoData.pages.length} 个分页
				</div>
			</div>

			<!-- 响应式布局：大屏幕左右布局，小屏幕上下布局 -->
			<div class="flex flex-col gap-6 xl:flex-row">
				<!-- 左侧/上方：分页列表 -->
				<div class="min-w-0 flex-1">
					<div
						class="grid gap-4"
						style="grid-template-columns: repeat(auto-fill, minmax(320px, 1fr));"
					>
						{#each videoData.pages as pageInfo, index (pageInfo.id)}
							<div class="space-y-3">
								<VideoCard
									video={{
										id: pageInfo.id,
										name: `P${pageInfo.pid}: ${pageInfo.name}`,
										upper_name: '',
										path: '',
										category: 0,
										cover: '',
										download_status: pageInfo.download_status
									}}
									mode="page"
									showActions={false}
									customTitle="P{pageInfo.pid}: {pageInfo.name}"
									customSubtitle=""
									taskNames={['视频封面', '视频内容', '视频信息', '视频弹幕', '视频字幕']}
									showProgress={false}
								/>

								<!-- 播放按钮区域 -->
								<div class="flex justify-center gap-2">
									{#if pageInfo.download_status[1] === 7}
										<Button
											size="sm"
											variant="default"
											class="flex-1"
											title="本地播放"
											onclick={() => {
												currentPlayingPageIndex = index;
												onlinePlayMode = false;
												showVideoPlayer = true;
											}}
										>
											<PlayIcon class="mr-2 h-4 w-4" />
											本地播放
										</Button>
									{/if}
									<Button
										size="sm"
										variant="outline"
										class="flex-1"
										title="在线播放"
										onclick={() => {
											currentPlayingPageIndex = index;
											onlinePlayMode = true;
											showVideoPlayer = true;
											const videoId = getPlayVideoId();
											loadOnlinePlayInfo(videoId);
										}}
									>
										<PlayIcon class="mr-2 h-4 w-4" />
										在线播放
									</Button>
								</div>

								<!-- 下载进度条 -->
								<div class="space-y-2 px-1">
									<div class="text-muted-foreground flex justify-between text-xs">
										<span class="truncate">下载进度</span>
										<span class="shrink-0"
											>{pageInfo.download_status.filter((s) => s === 7).length}/{pageInfo
												.download_status.length}</span
										>
									</div>
									<div class="flex w-full gap-1">
										{#each pageInfo.download_status as status, taskIndex (taskIndex)}
											<div
												class="h-2 w-full cursor-help rounded-sm transition-all {status === 7
													? 'bg-green-500'
													: status === 0
														? 'bg-yellow-500'
														: 'bg-red-500'}"
												title="{['视频封面', '视频内容', '视频信息', '视频弹幕', '视频字幕'][
													taskIndex
												]}: {status === 7 ? '已完成' : status === 0 ? '未开始' : `失败${status}次`}"
											></div>
										{/each}
									</div>
								</div>
							</div>
						{/each}
					</div>
				</div>

				<!-- 右侧/下方：视频播放器 -->
				{#if showVideoPlayer && videoData}
					<div class="w-full shrink-0 xl:w-[45%] 2xl:w-[40%]">
						<div class="sticky top-4">
							<div class="mb-4 flex items-center justify-between">
								<div class="flex items-center gap-2">
									<h3 class="text-lg font-semibold">视频播放</h3>
									<span
										class="rounded px-2 py-1 text-sm {onlinePlayMode
											? 'bg-blue-100 text-blue-700'
											: 'bg-gray-100 text-gray-700'}"
									>
										{onlinePlayMode ? '在线播放' : '本地播放'}
									</span>
									{#if onlinePlayMode && onlinePlayInfo}
										<span class="text-xs text-gray-500">
											{onlinePlayInfo.video_quality_description}
										</span>
										{#if isDashSeparatedStream()}
											<span class="text-xs text-green-600"> 视频+音频同步播放 </span>
										{/if}
									{/if}
								</div>
								<div class="flex items-center gap-2">
									<Button
										size="sm"
										variant="ghost"
										onclick={togglePlayMode}
										disabled={loadingPlayInfo}
									>
										{onlinePlayMode ? '切换到本地' : '切换到在线'}
									</Button>
									<Button size="sm" variant="outline" onclick={() => (showVideoPlayer = false)}>
										<XIcon class="mr-2 h-4 w-4" />
										关闭
									</Button>
								</div>
							</div>

							<!-- 当前播放的分页信息 -->
							{#if videoData.pages.length > 1}
								<div class="mb-2 text-sm text-gray-600">
									正在播放: P{videoData.pages[currentPlayingPageIndex].pid} - {videoData.pages[
										currentPlayingPageIndex
									].name}
								</div>
							{/if}

							<div class="overflow-hidden rounded-lg bg-black">
								{#if loadingPlayInfo && onlinePlayMode}
									<div class="flex h-64 items-center justify-center text-white">
										<div>加载播放信息中...</div>
									</div>
								{:else}
									{#key `${currentPlayingPageIndex}-${onlinePlayMode}`}
										<div
											class="video-container relative {onlinePlayMode ? 'online-mode' : ''}"
											role="group"
										>
											<video
												controls
												autoplay
												class="h-auto w-full"
												style="aspect-ratio: 16/9; max-height: 70vh;"
												src={getVideoSource()}
												crossorigin="anonymous"
												onerror={(e) => {
													console.warn('视频加载错误:', e);
												}}
												onloadstart={() => {
													console.log('开始加载视频:', getVideoSource());
												}}
												onplay={() => {
													// 同步播放音频
													if (isDashSeparatedStream()) {
														const audio = document.querySelector('#sync-audio');
														if (audio) audio.play();
													}
												}}
												onpause={() => {
													// 同步暂停音频
													if (isDashSeparatedStream()) {
														const audio = document.querySelector('#sync-audio');
														if (audio) audio.pause();
													}
												}}
												onseeked={() => {
													// 同步音频时间
													if (isDashSeparatedStream()) {
														const video = document.querySelector('video');
														const audio = document.querySelector('#sync-audio');
														if (video && audio) audio.currentTime = video.currentTime;
													}
												}}
												onvolumechange={() => {
													// 同步音量控制 - 固定100%音量
													if (isDashSeparatedStream()) {
														const video = document.querySelector('video');
														const audio = document.querySelector('#sync-audio');
														if (video && audio) {
															audio.volume = 1.0;
															audio.muted = video.muted;
														}
													}
												}}
												onloadedmetadata={() => {
													// 初始化时同步音量设置 - 固定100%音量
													if (isDashSeparatedStream()) {
														const video = document.querySelector('video');
														const audio = document.querySelector('#sync-audio');
														if (video && audio) {
															audio.volume = 1.0;
															audio.muted = video.muted;
														}
														// 初始化音频同步
														initAudioSync();
													}
												}}
											>
												<!-- 默认空字幕轨道用于无障碍功能 -->
												<track kind="captions" srclang="zh" label="无字幕" default />
												{#if onlinePlayMode && onlinePlayInfo && onlinePlayInfo.subtitle_streams}
													{#each onlinePlayInfo.subtitle_streams as subtitle}
														<track
															kind="subtitles"
															srclang={subtitle.language}
															label={subtitle.language_doc}
															src={subtitle.url}
														/>
													{/each}
												{/if}
												您的浏览器不支持视频播放。
											</video>

											<!-- 隐藏的音频元素用于DASH分离流 -->
											{#if isDashSeparatedStream()}
												<audio
													id="sync-audio"
													src={getAudioSource()}
													crossorigin="anonymous"
													style="display: none;"
												></audio>
											{/if}
										</div>
									{/key}
								{/if}
							</div>

							<!-- 分页选择按钮 -->
							{#if videoData.pages.length > 1}
								<div class="mt-4 space-y-2">
									<div class="text-sm font-medium text-gray-700">选择分页:</div>
									<div class="grid max-h-60 grid-cols-2 gap-2 overflow-y-auto">
										{#each videoData.pages as page, index}
											{#if page.download_status[1] === 7}
												<Button
													size="sm"
													variant={currentPlayingPageIndex === index ? 'default' : 'outline'}
													class="justify-start text-left"
													onclick={() => {
														currentPlayingPageIndex = index;
														// 如果是在线播放模式，需要重新获取播放信息
														if (onlinePlayMode) {
															const videoId = getPlayVideoId();
															loadOnlinePlayInfo(videoId);
														} else {
															// 本地播放模式：强制重新加载视频
															setTimeout(() => {
																const videoElement = document.querySelector('video');
																if (videoElement) {
																	try {
																		videoElement.load();
																	} catch (e) {
																		console.warn('视频重载失败:', e);
																	}
																}
															}, 100);
														}
													}}
												>
													<span class="truncate">P{page.pid}: {page.name}</span>
												</Button>
											{/if}
										{/each}
									</div>
								</div>
							{/if}
						</div>
					</div>
				{/if}
			</div>
		{:else}
			<div class="py-12 text-center">
				<div class="space-y-2">
					<p class="text-muted-foreground">暂无分P数据</p>
					<p class="text-muted-foreground text-sm">该视频可能为单P视频</p>
				</div>
			</div>
		{/if}
	</section>

	<!-- 状态编辑器 -->
	{#if videoData}
		<StatusEditor
			bind:open={statusEditorOpen}
			video={videoData.video}
			pages={videoData.pages}
			loading={statusEditorLoading}
			onsubmit={handleStatusEditorSubmit}
		/>
	{/if}

	<!-- 删除确认对话框 -->
	{#if deleteDialogOpen}
		<div class="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
			<div class="bg-background mx-4 w-full max-w-md rounded-lg border p-6 shadow-lg">
				<div class="space-y-4">
					<div class="space-y-2">
						<h3 class="text-lg font-semibold">确认删除视频</h3>
						<p class="text-muted-foreground">
							确定要删除视频 "<span class="font-medium">{videoData?.video.name}</span>" 吗？
						</p>
						<p class="text-muted-foreground text-sm">
							此操作将把视频标记为已删除状态，不会删除实际文件。在视频源设置中开启"扫描已删除视频"后可重新下载。
						</p>
					</div>
					<div class="flex justify-end gap-2">
						<Button
							variant="outline"
							onclick={() => (deleteDialogOpen = false)}
							disabled={deleting}
						>
							取消
						</Button>
						<Button variant="destructive" onclick={handleDeleteVideo} disabled={deleting}>
							{deleting ? '删除中...' : '确认删除'}
						</Button>
					</div>
				</div>
			</div>
		</div>
	{/if}
{/if}

<style>
	/* 在线播放时隐藏原生音量控制 */
	.video-container.online-mode video::-webkit-media-controls-volume-control-container {
		display: none !important;
	}

	.video-container.online-mode video::-webkit-media-controls-mute-button {
		display: none !important;
	}

	.video-container.online-mode video::-moz-volume-control {
		display: none !important;
	}

	/* 视频容器 */
	.video-container {
		position: relative;
	}
</style>
