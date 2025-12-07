<script lang="ts">
	import { goto } from '$app/navigation';
	import { page } from '$app/stores';
	import api from '$lib/api';
	import StatusEditor from '$lib/components/status-editor.svelte';
	import { Button } from '$lib/components/ui/button/index.js';
	import VideoCard from '$lib/components/video-card.svelte';
	import { setBreadcrumb } from '$lib/stores/breadcrumb';
	import { appStateStore, setVideoIds, setCurrentPage, setVideoListInfo, setTotalCount, ToQuery } from '$lib/stores/filter';
	import type { ApiError, UpdateVideoStatusRequest, VideoResponse } from '$lib/types';
	import ChevronLeftIcon from '@lucide/svelte/icons/chevron-left';
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import EditIcon from '@lucide/svelte/icons/edit';
	import PlayIcon from '@lucide/svelte/icons/play';
	import TrashIcon from '@lucide/svelte/icons/trash-2';
	import XIcon from '@lucide/svelte/icons/x';
	import { onMount } from 'svelte';
	import { toast } from 'svelte-sonner';
	import { get } from 'svelte/store';

	let videoData: VideoResponse | null = null;
	let loading = false;
	let error: string | null = null;
	// let _resetDialogOpen = false; // 未使用，已注释
	// let _resetting = false; // 未使用，已注释
	let statusEditorOpen = false;
	let statusEditorLoading = false;
	let showVideoPlayer = false;
	let currentPlayingPageIndex = 0;
	let onlinePlayMode = false; // false: 本地播放, true: 在线播放
	// eslint-disable-next-line @typescript-eslint/no-explicit-any
	let onlinePlayInfo: any = null;
	let loadingPlayInfo = false;
	// let _isFullscreen = false; // 未使用，已注释 // 是否全屏模式
	let deleteDialogOpen = false;
	let deleting = false;

	// 响应式相关
	let innerWidth: number;
	let isMobile: boolean = false;
	$: isMobile = innerWidth < 768; // sm断点

	// 视频导航相关
	$: currentVideoId = videoData?.video.id ?? 0;
	$: videoIds = $appStateStore.videoIds;
	$: totalCount = $appStateStore.totalCount;
	$: pageSize = $appStateStore.pageSize;
	$: currentPage = $appStateStore.currentPage;
	$: currentIndexInPage = videoIds.indexOf(currentVideoId);
	$: globalIndex = currentPage * pageSize + currentIndexInPage; // 全局索引
	$: totalPages = Math.ceil(totalCount / pageSize);
	$: hasPrevVideo = globalIndex > 0;
	$: hasNextVideo = globalIndex < totalCount - 1;
	let navigating = false;

	async function goToPrevVideo() {
		if (!hasPrevVideo || navigating) return;

		navigating = true;
		try {
			if (currentIndexInPage > 0) {
				// 当前页内有上一个视频
				const prevVideoId = videoIds[currentIndexInPage - 1];
				goto(`/video/${prevVideoId}`);
			} else if (currentPage > 0) {
				// 需要加载上一页
				const prevPage = currentPage - 1;
				await loadPageVideos(prevPage);
				// 跳转到上一页的最后一个视频
				const state = get(appStateStore);
				if (state.videoIds.length > 0) {
					const lastVideoId = state.videoIds[state.videoIds.length - 1];
					goto(`/video/${lastVideoId}`);
				}
			}
		} finally {
			navigating = false;
		}
	}

	async function goToNextVideo() {
		if (!hasNextVideo || navigating) return;

		navigating = true;
		try {
			if (currentIndexInPage < videoIds.length - 1) {
				// 当前页内有下一个视频
				const nextVideoId = videoIds[currentIndexInPage + 1];
				goto(`/video/${nextVideoId}`);
			} else if (currentPage < totalPages - 1) {
				// 需要加载下一页
				const nextPage = currentPage + 1;
				await loadPageVideos(nextPage);
				// 跳转到下一页的第一个视频
				const state = get(appStateStore);
				if (state.videoIds.length > 0) {
					const firstVideoId = state.videoIds[0];
					goto(`/video/${firstVideoId}`);
				}
			}
		} finally {
			navigating = false;
		}
	}

	async function loadPageVideos(pageNum: number) {
		const state = get(appStateStore);
		const params: Record<string, string | number | boolean> = {
			page: pageNum,
			page_size: state.pageSize,
			sort_by: state.sortBy,
			sort_order: state.sortOrder
		};
		if (state.query) {
			params.query = state.query;
		}
		if (state.videoSource) {
			params[state.videoSource.type] = parseInt(state.videoSource.id);
		}
		if (state.showFailedOnly) {
			params.show_failed_only = true;
		}

		const result = await api.getVideos(params);
		setCurrentPage(pageNum);
		setVideoListInfo(
			result.data.videos.map((v) => v.id),
			result.data.total_count,
			state.pageSize
		);
	}

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
	// eslint-disable-next-line @typescript-eslint/no-unused-vars, @typescript-eslint/no-explicit-any
	function _isVideoPlayable(video: any): boolean {
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
				label: '视频管理',
				onClick: () => {
					const query = ToQuery($appStateStore);
					goto(query ? `/videos?${query}` : '/videos');
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

	// 打开B站页面
	async function openBilibiliPage() {
		try {
			const videoId = getPlayVideoId();
			const result = await api.getVideoPlayInfo(videoId);
			const bilibiliUrl = result.data.bilibili_url;

			if (bilibiliUrl) {
				console.log('获取到B站链接:', bilibiliUrl);
				window.open(bilibiliUrl, '_blank');
			} else if (result.data.video_bvid) {
				// 如果没有bilibili_url但有bvid，手动构建链接
				const manualUrl = `https://www.bilibili.com/video/${result.data.video_bvid}`;
				console.log('手动构建B站链接:', manualUrl);
				window.open(manualUrl, '_blank');
			} else {
				throw new Error('无法获取视频的B站标识信息');
			}
		} catch (error) {
			console.error('获取B站链接失败:', error);
			toast.error('无法获取B站链接', {
				description: '该视频可能没有有效的B站链接信息'
			});
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
			// eslint-disable-next-line @typescript-eslint/no-explicit-any
			(document as any).webkitFullscreenElement ||
			// eslint-disable-next-line @typescript-eslint/no-explicit-any
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
			const currentVideoId = videoData.video.id;
			const result = await api.deleteVideo(currentVideoId);
			const data = result.data;

			if (data.success) {
				toast.success('视频删除成功', {
					description: '视频已被标记为删除状态'
				});
				deleteDialogOpen = false;

				// 获取当前视频列表，找到下一个视频
				const state = get(appStateStore);
				const oldVideoIds = state.videoIds;
				const currentIndex = oldVideoIds.indexOf(currentVideoId);

				if (currentIndex !== -1 && oldVideoIds.length > 1) {
					// 先确定要跳转到的视频ID（在移除当前视频之前）
					// 优先跳转到下一个视频，如果是最后一个则跳转到上一个
					let targetVideoId: number;
					if (currentIndex < oldVideoIds.length - 1) {
						// 不是最后一个，跳转到下一个
						targetVideoId = oldVideoIds[currentIndex + 1];
					} else {
						// 是最后一个，跳转到上一个
						targetVideoId = oldVideoIds[currentIndex - 1];
					}

					// 从列表中移除当前视频
					const newVideoIds = oldVideoIds.filter((id) => id !== currentVideoId);
					setVideoIds(newVideoIds);
					// 更新总数
					setTotalCount(state.totalCount - 1);

					// 跳转到目标视频
					goto(`/video/${targetVideoId}`);
				} else {
					// 列表为空或没有更多视频，返回视频管理页面
					const query = ToQuery(state);
					goto(query ? `/videos?${query}` : '/videos');
				}
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

<svelte:window bind:innerWidth />

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
		<div class="mb-4 flex {isMobile ? 'flex-col gap-3' : 'items-center justify-between'}">
			<div class="flex items-center gap-2">
				{#if totalCount > 1}
					<Button
						size="sm"
						variant="outline"
						class="cursor-pointer"
						onclick={goToPrevVideo}
						disabled={!hasPrevVideo || navigating}
						title="上一个视频"
					>
						<ChevronLeftIcon class="h-4 w-4" />
					</Button>
				{/if}
				<h2 class="{isMobile ? 'text-lg' : 'text-xl'} font-semibold">视频信息</h2>
				{#if totalCount > 1}
					<Button
						size="sm"
						variant="outline"
						class="cursor-pointer"
						onclick={goToNextVideo}
						disabled={!hasNextVideo || navigating}
						title="下一个视频"
					>
						<ChevronRightIcon class="h-4 w-4" />
					</Button>
					<span class="text-muted-foreground text-sm">
						{globalIndex + 1} / {totalCount}
					</span>
				{/if}
			</div>
			<div class="flex {isMobile ? 'flex-col gap-2' : 'gap-2'}">
				<Button
					size="sm"
					variant="outline"
					class="{isMobile ? 'w-full' : 'shrink-0'} cursor-pointer"
					onclick={openBilibiliPage}
					title="在B站打开此视频"
				>
					<svg class="mr-2 h-4 w-4" viewBox="0 0 24 24" fill="currentColor">
						<path
							d="M9.64 7.64c.23-.5.36-1.05.36-1.64 0-2.21-1.79-4-4-4S2 3.79 2 6s1.79 4 4 4c.59 0 1.14-.13 1.64-.36L10 12l-2.36 2.36c-.5-.23-1.05-.36-1.64-.36-2.21 0-4 1.79-4 4s1.79 4 4 4 4-1.79 4-4c0-.59-.13-1.14-.36-1.64L12 14l2.36 2.36c-.23.5-.36 1.05-.36 1.64 0 2.21 1.79 4 4 4s4-1.79 4-4-1.79-4-4-4c-.59 0-1.14.13-1.64.36L14 12l2.36-2.36c.5.23 1.05.36 1.64.36 2.21 0 4-1.79 4-4s-1.79-4-4-4-4 1.79-4 4c0 .59.13 1.14.36 1.64L12 10 9.64 7.64z"
						/>
					</svg>
					访问B站
				</Button>
				<Button
					size="sm"
					variant="outline"
					class="{isMobile ? 'w-full' : 'shrink-0'} cursor-pointer"
					onclick={() => (statusEditorOpen = true)}
					disabled={statusEditorLoading}
				>
					<EditIcon class="mr-2 h-4 w-4" />
					编辑状态
				</Button>
				<Button
					size="sm"
					variant="destructive"
					class="{isMobile ? 'w-full' : 'shrink-0'} cursor-pointer"
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
			<div class="bg-muted mb-4 rounded-lg border {isMobile ? 'p-3' : 'p-4'}">
				<h3 class="text-foreground mb-2 text-sm font-medium">📁 下载保存路径</h3>
				<div
					class="bg-card rounded border {isMobile ? 'px-2 py-2' : 'px-3 py-2'} font-mono {isMobile
						? 'text-xs'
						: 'text-sm'} break-all"
				>
					{videoData.pages[0].path}
				</div>
				<p class="text-muted-foreground mt-1 text-xs">视频文件将保存到此路径下</p>
			</div>
		{/if}
	</section>

	<section>
		{#if videoData.pages && videoData.pages.length > 0}
			<div class="mb-4 flex {isMobile ? 'flex-col gap-2' : 'items-center justify-between'}">
				<h2 class="{isMobile ? 'text-lg' : 'text-xl'} font-semibold">分页列表</h2>
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
						style="grid-template-columns: repeat(auto-fill, minmax({isMobile
							? '280px'
							: '320px'}, 1fr));"
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
												onseeked={(e) => {
													// 同步音频时间
													if (isDashSeparatedStream()) {
														const video = e.currentTarget as HTMLVideoElement;
														const audio = document.querySelector('#sync-audio') as HTMLAudioElement;
														if (video && audio) audio.currentTime = video.currentTime;
													}
												}}
												onvolumechange={(e) => {
													// 同步音量控制 - 固定100%音量
													if (isDashSeparatedStream()) {
														const video = e.currentTarget as HTMLVideoElement;
														const audio = document.querySelector('#sync-audio') as HTMLAudioElement;
														if (video && audio) {
															audio.volume = 1.0;
															audio.muted = video.muted;
														}
													}
												}}
												onloadedmetadata={(e) => {
													// 初始化时同步音量设置 - 固定100%音量
													if (isDashSeparatedStream()) {
														const video = e.currentTarget as HTMLVideoElement;
														const audio = document.querySelector('#sync-audio') as HTMLAudioElement;
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
													{#each onlinePlayInfo.subtitle_streams as subtitle (subtitle.language)}
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
										{#each videoData.pages as page, index (page.id)}
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
							删除当前视频后，在视频源设置中开启"扫描已删除视频"后可重新下载。
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
