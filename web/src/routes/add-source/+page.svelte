<script lang="ts">
	import { goto } from '$app/navigation';
	import api from '$lib/api';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { setBreadcrumb } from '$lib/stores/breadcrumb';
	import type {
		SearchResultItem,
		VideoCategory,
		SubmissionVideoInfo,
		UserFavoriteFolder,
		UserCollectionItem,
		UserFollowing,
		BangumiSeasonInfo,
		BangumiSourceOption,
		BangumiSourceListResponse,
		ValidateFavoriteResponse,
		UserCollectionInfo,
		AddVideoSourceRequest,
		KeywordFilterMode
	} from '$lib/types';
	import { Search, X, Plus as PlusIcon, Filter as FilterIcon } from '@lucide/svelte';
	import { onDestroy, onMount } from 'svelte';
	import { toast } from 'svelte-sonner';
	import { flip } from 'svelte/animate';
	import { fade, fly } from 'svelte/transition';

	let sourceType: VideoCategory = 'collection';
	let lastSourceType: VideoCategory = sourceType; // 记录上一次的源类型，用于检测切换
	let sourceId = '';
	let upId = '';
	let name = '';
	let path = '';
	let cover = '';
	let collectionType = 'season';
	let downloadAllSeasons = false;
	let loading = false;

	// 添加手动输入标志
	let isManualInput = false;

	// 搜索相关
	let searchKeyword = '';
	let searchLoading = false;
	let searchResults: SearchResultItem[] = [];
	let showSearchResults = false;

	let searchTotalResults = 0;

	// 收藏夹相关
	let userFavorites: UserFavoriteFolder[] = [];
	let loadingFavorites = false;
	let validatingFavorite = false;
	let favoriteValidationResult: ValidateFavoriteResponse | null = null;
	let favoriteValidationTimeout: ReturnType<typeof setTimeout> | null = null;

	// UP主收藏夹搜索相关
	let searchedUserFavorites: UserFavoriteFolder[] = [];
	let loadingSearchedUserFavorites = false;
	let selectedUserId: string = '';
	let selectedUserName: string = '';

	// UP主合集相关
	let userCollections: UserCollectionItem[] = [];
	// eslint-disable-next-line @typescript-eslint/no-unused-vars
	let loadingCollections = false; // 合集加载状态
	let upIdTimeout: ReturnType<typeof setTimeout> | null = null;

	// 关注的UP主相关
	let userFollowings: UserFollowing[] = [];
	let loadingFollowings = false;

	// 番剧季度相关
	let bangumiSeasons: BangumiSeasonInfo[] = [];
	let loadingSeasons = false;
	let selectedSeasons: string[] = [];
	let seasonIdTimeout: ReturnType<typeof setTimeout> | null = null;

	// 番剧合并相关
	let existingBangumiSources: BangumiSourceOption[] = [];
	let loadingBangumiSources = false;
	let mergeToSourceId: number | null = null;
	let showMergeOptions = false;
	let cachedNameBeforeMerge = '';
	let cachedPathBeforeMerge = '';
	let isUsingMergedSourceMeta = false;

	// 过滤已有视频源相关
	let existingVideoSources: VideoSourcesResponse | null = null;
	let existingCollectionIds: Set<string> = new Set();
	let existingFavoriteIds: Set<number> = new Set();
	let existingSubmissionIds: Set<number> = new Set();
	let existingBangumiSeasonIds: Set<string> = new Set();
	let loadingExistingSources = false;
	let isMergingBangumi = false;

	// 关键词过滤器相关（双列表模式）
	let blacklistKeywords: string[] = [];
	let whitelistKeywords: string[] = [];
	let newBlacklistKeyword = '';
	let newWhitelistKeyword = '';
	let blacklistValidationError = '';
	let whitelistValidationError = '';
	let validatingBlacklistKeyword = false;
	let validatingWhitelistKeyword = false;
	let showKeywordSection = false; // 是否展开关键词过滤器部分

	// 批量添加相关
	let batchMode = false; // 是否为批量模式
	let batchSelectedItems = new Map(); // 存储选中项 {key: {type, data, name}}
	let batchCheckboxStates = {}; // 存储checkbox状态的响应式对象
	let batchBasePath = '/Downloads'; // 批量基础路径
	let batchAdding = false; // 批量添加进行中
	let batchProgress = { current: 0, total: 0 }; // 批量添加进度
	let batchDialogOpen = false; // 批量配置对话框状态

	// 响应式语句：当Map变化时更新checkbox状态对象
	$: {
		const newStates = {};
		for (const [key] of batchSelectedItems) {
			newStates[key] = true;
		}
		batchCheckboxStates = newStates;
		console.log('🔄 Reactive update - checkbox states:', Object.keys(newStates));
	}

	// 悬停详情相关
	let hoveredItem: {
		type: 'search' | 'season';
		data: SearchResultItem | BangumiSeasonInfo;
	} | null = null;
	let hoverTimeout: ReturnType<typeof setTimeout> | null = null;
	let mousePosition = { x: 0, y: 0 };

	// 响应式相关
	let innerWidth: number;
	let isMobile: boolean = false;
	$: isMobile = innerWidth < 768; // md断点

	// 源类型选项
	const sourceTypeOptions = [
		{ value: 'collection', label: '合集', description: '视频合集，需要UP主ID和合集ID' },
		{
			value: 'favorite',
			label: '收藏夹',
			description: '可添加任何公开收藏夹，收藏夹ID可在收藏夹页面URL中获取'
		},
		{ value: 'submission', label: 'UP主投稿', description: 'UP主ID可在UP主空间URL中获取' },
		{ value: 'watch_later', label: '稍后观看', description: '同步稍后观看列表' },
		{ value: 'bangumi', label: '番剧', description: '番剧season_id可在番剧页面URL中获取' }
	];

	// 合集类型选项
	const collectionTypeOptions = [
		{ value: 'season', label: '合集', description: 'B站标准合集' },
		{ value: 'series', label: '系列', description: '视频系列' }
	];

	// 订阅的合集相关
	let subscribedCollections: UserCollectionInfo[] = [];
	let loadingSubscribedCollections = false;

	// UP主投稿选择相关
	let showSubmissionSelection = false;
	let selectedVideos: string[] = [];
	// eslint-disable-next-line @typescript-eslint/no-unused-vars
	let selectedUpName = ''; // UP主名称，用于投稿选择

	// 投稿选择详细状态
	let submissionVideos: SubmissionVideoInfo[] = [];
	let selectedSubmissionVideos: Set<string> = new Set();
	let submissionLoading = false;
	let submissionError: string | null = null;
	let submissionTotalCount = 0;
	let submissionSearchQuery = '';
	let filteredSubmissionVideos: SubmissionVideoInfo[] = [];

	// 分页加载相关状态
	let currentLoadedPage = 0; // 当前加载到的页码
	let isLoadingMore = false; // 正在加载更多
	let hasMoreVideos = true; // 是否还有更多视频
	// eslint-disable-next-line @typescript-eslint/no-unused-vars
	let loadingProgress = ''; // 加载进度提示
	let showLoadMoreButton = false; // 是否显示加载更多按钮

	const SUBMISSION_PAGE_SIZE = 20;
	const INITIAL_LOAD_SIZE = 100; // 初始加载100个视频
	const LOAD_MORE_SIZE = 200; // 每次加载更多200个视频
	const PAGE_DELAY = 500; // 页面间延迟500ms

	// 滚动容器引用
	let submissionScrollContainer: HTMLElement;

	onMount(async () => {
		setBreadcrumb([
			{ label: '主页', href: '/' },
			{ label: '添加视频源', isActive: true }
		]);
		await loadExistingVideoSources();
	});

	onDestroy(() => {
		// 清理定时器
		clearTimeout(hoverTimeout);
		clearTimeout(upIdTimeout);
		clearTimeout(seasonIdTimeout);
		clearTimeout(favoriteValidationTimeout);
	});

	$: isMergingBangumi = sourceType === 'bangumi' && mergeToSourceId !== null;

	$: {
		if (isMergingBangumi) {
			const targetSource = existingBangumiSources.find((source) => source.id === mergeToSourceId);
			if (targetSource) {
				if (!isUsingMergedSourceMeta) {
					cachedNameBeforeMerge = name;
					cachedPathBeforeMerge = path;
				}
				name = targetSource.name;
				path = targetSource.path;
				isUsingMergedSourceMeta = true;
			}
		} else if (isUsingMergedSourceMeta) {
			name = cachedNameBeforeMerge;
			path = cachedPathBeforeMerge;
			isUsingMergedSourceMeta = false;
		}
	}

	// 搜索B站内容
	async function handleSearch(overrideSearchType?: string) {
		if (!searchKeyword.trim()) {
			toast.error('请输入搜索关键词');
			return;
		}

		// 根据参数或当前选择的视频源类型确定搜索类型
		let searchType: 'video' | 'bili_user' | 'media_bangumi';
		if (overrideSearchType) {
			searchType = overrideSearchType as 'video' | 'bili_user' | 'media_bangumi';
		} else {
			switch (sourceType) {
				case 'collection':
				case 'submission':
				case 'favorite': // 收藏夹类型也搜索UP主
					searchType = 'bili_user';
					break;
				case 'bangumi':
					searchType = 'media_bangumi';
					break;
				default:
					searchType = 'video';
					break;
			}
		}

		searchLoading = true;
		searchResults = [];
		searchTotalResults = 0;

		try {
			// 针对番剧搜索，需要更多页面因为每页实际只有25+25=50个结果但分配可能不均
			const pageSize = searchType === 'media_bangumi' ? 100 : 50;

			// 第一次请求获取总数
			const firstResult = await api.searchBilibili({
				keyword: searchKeyword,
				search_type: searchType,
				page: 1,
				page_size: pageSize
			});

			if (!firstResult.data.success) {
				toast.error('搜索失败');
				return;
			}

			const totalResults = firstResult.data.total;
			searchTotalResults = totalResults;
			let allResults = [...firstResult.data.results];

			// 如果总数超过pageSize，继续获取剩余页面
			if (totalResults > pageSize) {
				const totalPages = Math.ceil(totalResults / pageSize);
				const remainingPages = Array.from({ length: totalPages - 1 }, (_, i) => i + 2);

				// 串行获取剩余页面，避免并发请求过多导致失败
				for (const page of remainingPages) {
					try {
						const pageResult = await api.searchBilibili({
							keyword: searchKeyword,
							search_type: searchType,
							page,
							page_size: pageSize
						});

						if (pageResult.data.success && pageResult.data.results) {
							allResults.push(...pageResult.data.results);
						}

						// 添加小延迟避免请求过于频繁
						await new Promise((resolve) => setTimeout(resolve, 100));
					} catch {
						// 静默处理失败，继续获取下一页
					}
				}
			}

			// 去重处理（基于season_id, bvid, mid等唯一标识）
			const uniqueResults = allResults.filter((result, index, arr) => {
				const id = result.season_id || result.bvid || result.mid || `${result.title}_${index}`;
				return (
					arr.findIndex((r) => {
						const rid = r.season_id || r.bvid || r.mid || `${r.title}_${arr.indexOf(r)}`;
						return rid === id;
					}) === index
				);
			});

			searchResults = uniqueResults;
			showSearchResults = true;

			// 优化提示信息
			const successRate = ((uniqueResults.length / totalResults) * 100).toFixed(1);
			if (uniqueResults.length < totalResults) {
				toast.success(
					`搜索完成，获取到 ${uniqueResults.length}/${totalResults} 个结果 (${successRate}%)`
				);
			} else {
				toast.success(`搜索完成，共获取到 ${uniqueResults.length} 个结果`);
			}
		} catch (error: unknown) {
			console.error('搜索失败:', error);
			const errorMessage = error instanceof Error ? error.message : '搜索失败';
			toast.error('搜索失败', { description: errorMessage });
		} finally {
			searchLoading = false;
		}
	}

	// 选择搜索结果
	function selectSearchResult(result: SearchResultItem) {
		switch (sourceType) {
			case 'collection':
				if (result.mid) {
					upId = result.mid.toString();
					// 触发获取UP主合集列表
					handleUpIdChange();
					toast.success('已填充UP主信息', { description: '正在获取合集列表...' });
				}
				break;
			case 'submission':
				if (result.mid) {
					sourceId = result.mid.toString();
					name = cleanTitle(result.title);
					selectedUpName = cleanTitle(result.title);
					// 打开投稿选择对话框
					showSubmissionSelection = true;
				}
				break;
			case 'bangumi':
				if (result.season_id) {
					sourceId = result.season_id;
					name = cleanTitle(result.title);
				}
				break;
			case 'favorite':
				// 收藏夹类型搜索UP主，调用获取收藏夹函数
				if (result.mid) {
					selectUserAndFetchFavorites(result);
					return; // 直接返回，不执行后续逻辑
				}
				break;
			default:
				if (result.bvid) {
					sourceId = result.bvid;
					name = cleanTitle(result.title);
				}
				break;
		}

		// 关闭搜索结果
		showSearchResults = false;
		searchResults = [];
		searchKeyword = '';
		searchTotalResults = 0;

		// 清除悬停状态
		hoveredItem = null;

		if (sourceType !== 'collection') {
			toast.success('已填充信息', { description: '请检查并完善其他必要信息' });
		}
	}

	// 清理标题中的HTML标签
	function cleanTitle(title: string): string {
		const div = document.createElement('div');
		div.innerHTML = title;
		return div.textContent || div.innerText || title;
	}

	// 检查关键词是否在另一个列表中存在（互斥校验）
	function checkMutualExclusivity(keyword: string, targetList: 'blacklist' | 'whitelist'): string | null {
		if (targetList === 'blacklist' && whitelistKeywords.includes(keyword)) {
			return '该关键词已存在于白名单中，同一关键词不能同时出现在黑名单和白名单';
		}
		if (targetList === 'whitelist' && blacklistKeywords.includes(keyword)) {
			return '该关键词已存在于黑名单中，同一关键词不能同时出现在黑名单和白名单';
		}
		return null;
	}

	// 添加黑名单关键词
	async function addBlacklistKeyword() {
		const pattern = newBlacklistKeyword.trim();
		if (!pattern) {
			blacklistValidationError = '请输入关键词';
			return;
		}

		if (blacklistKeywords.includes(pattern)) {
			blacklistValidationError = '该关键词已存在于黑名单中';
			return;
		}

		// 互斥校验
		const mutualError = checkMutualExclusivity(pattern, 'blacklist');
		if (mutualError) {
			blacklistValidationError = mutualError;
			return;
		}

		// 验证正则表达式
		validatingBlacklistKeyword = true;
		try {
			const result = await api.validateRegex(pattern);
			if (result.status_code === 200) {
				if (result.data.valid) {
					blacklistKeywords = [...blacklistKeywords, pattern];
					newBlacklistKeyword = '';
					blacklistValidationError = '';
				} else {
					blacklistValidationError = result.data.error || '无效的正则表达式';
				}
			} else {
				blacklistValidationError = '验证请求失败';
			}
		} catch {
			blacklistValidationError = '网络错误';
		} finally {
			validatingBlacklistKeyword = false;
		}
	}

	// 添加白名单关键词
	async function addWhitelistKeyword() {
		const pattern = newWhitelistKeyword.trim();
		if (!pattern) {
			whitelistValidationError = '请输入关键词';
			return;
		}

		if (whitelistKeywords.includes(pattern)) {
			whitelistValidationError = '该关键词已存在于白名单中';
			return;
		}

		// 互斥校验
		const mutualError = checkMutualExclusivity(pattern, 'whitelist');
		if (mutualError) {
			whitelistValidationError = mutualError;
			return;
		}

		// 验证正则表达式
		validatingWhitelistKeyword = true;
		try {
			const result = await api.validateRegex(pattern);
			if (result.status_code === 200) {
				if (result.data.valid) {
					whitelistKeywords = [...whitelistKeywords, pattern];
					newWhitelistKeyword = '';
					whitelistValidationError = '';
				} else {
					whitelistValidationError = result.data.error || '无效的正则表达式';
				}
			} else {
				whitelistValidationError = '验证请求失败';
			}
		} catch {
			whitelistValidationError = '网络错误';
		} finally {
			validatingWhitelistKeyword = false;
		}
	}

	// 删除黑名单关键词
	function removeBlacklistKeyword(index: number) {
		blacklistKeywords = blacklistKeywords.filter((_, i) => i !== index);
	}

	// 删除白名单关键词
	function removeWhitelistKeyword(index: number) {
		whitelistKeywords = whitelistKeywords.filter((_, i) => i !== index);
	}

	// 处理黑名单关键词输入框键盘事件
	function handleBlacklistKeywordKeydown(event: KeyboardEvent) {
		if (event.key === 'Enter') {
			event.preventDefault();
			addBlacklistKeyword();
		}
	}

	// 处理白名单关键词输入框键盘事件
	function handleWhitelistKeywordKeydown(event: KeyboardEvent) {
		if (event.key === 'Enter') {
			event.preventDefault();
			addWhitelistKeyword();
		}
	}

	// 处理图片URL
	function processBilibiliImageUrl(url: string): string {
		if (!url) return '';

		if (url.startsWith('https://')) return url;
		if (url.startsWith('//')) return 'https:' + url;
		if (url.startsWith('http://')) return url.replace('http://', 'https://');
		if (!url.startsWith('http')) return 'https://' + url;

		return url.split('@')[0];
	}

	// 处理图片加载错误
	function handleImageError(event: Event) {
		const img = event.target as HTMLImageElement;
		// 使用默认的占位图片
		img.style.display = 'none';
		const parent = img.parentElement;
		if (parent && !parent.querySelector('.placeholder')) {
			const placeholder = document.createElement('div');
			// 获取原图片的尺寸类
			const widthClass = img.className.match(/w-\d+/)?.[0] || 'w-20';
			const heightClass = img.className.match(/h-\d+/)?.[0] || 'h-14';
			placeholder.className = `placeholder ${widthClass} ${heightClass} bg-muted rounded flex items-center justify-center text-xs text-muted-foreground`;
			placeholder.textContent = '无图片';
			parent.appendChild(placeholder);
		}
	}

	async function handleSubmit() {
		// 验证表单
		if (sourceType !== 'watch_later' && !sourceId) {
			toast.error('请输入ID', { description: '视频源ID不能为空' });
			return;
		}

		if (sourceType === 'collection' && !upId) {
			toast.error('请输入UP主ID', { description: '合集需要提供UP主ID' });
			return;
		}

		if (!name) {
			toast.error('请输入名称', { description: '视频源名称不能为空' });
			return;
		}

		if (!path) {
			toast.error('请输入保存路径', { description: '保存路径不能为空' });
			return;
		}

		// 番剧特殊验证
		if (sourceType === 'bangumi') {
			// 如果不是下载全部季度，且没有选择任何季度，且不是单季度情况，则提示错误
			if (!downloadAllSeasons && selectedSeasons.length === 0 && bangumiSeasons.length > 1) {
				toast.error('请选择要下载的季度', {
					description: '未选择"下载全部季度"时，至少需要选择一个季度'
				});
				return;
			}
		}

		loading = true;

		try {
			const params: AddVideoSourceRequest = {
				source_type: sourceType,
				source_id: sourceId,
				name,
				path
			};

			if (sourceType === 'collection') {
				params.up_id = upId;
				params.collection_type = collectionType;
				if (cover) {
					params.cover = cover;
				}
			}

			if (sourceType === 'bangumi') {
				params.download_all_seasons = downloadAllSeasons;
				// 如果选择了特定季度，添加selected_seasons参数
				if (selectedSeasons.length > 0 && !downloadAllSeasons) {
					params.selected_seasons = selectedSeasons;
				}
				// 如果选择了合并到现有番剧源，添加merge_to_source_id参数
				if (mergeToSourceId) {
					params.merge_to_source_id = mergeToSourceId;
				}
			}

			if (sourceType === 'submission') {
				// 如果有选择的视频，添加selected_videos参数
				if (selectedVideos.length > 0) {
					params.selected_videos = selectedVideos;
				}
			}

			// 如果有关键词过滤器，添加blacklist_keywords和whitelist_keywords参数（双列表模式）
			// 注意：后端API仍使用keyword_filters和keyword_filter_mode，但会被转换为新格式
			// 为了向后兼容，在添加时使用旧格式，后端会自动处理
			if (blacklistKeywords.length > 0 || whitelistKeywords.length > 0) {
				// 使用新的双列表模式，直接传递两个列表
				// 后端handler会根据是否存在这些字段来决定使用哪种模式
				if (blacklistKeywords.length > 0) {
					params.keyword_filters = blacklistKeywords;
					params.keyword_filter_mode = 'blacklist';
				}
				// 注意：当前添加接口只支持单一模式，双列表需要后续通过编辑接口设置
				// 如果同时有白名单，需要先添加视频源，然后再通过关键词过滤器编辑功能设置完整的双列表
			}

			const result = await api.addVideoSource(params);

			if (result.data.success) {
				// 如果同时设置了白名单，需要额外调用API更新
				if (whitelistKeywords.length > 0 && result.data.source_id) {
					try {
						await api.updateVideoSourceKeywordFilters(
							sourceType,
							result.data.source_id,
							blacklistKeywords,
							whitelistKeywords
						);
					} catch (e) {
						console.warn('更新关键词过滤器失败:', e);
					}
				}

				toast.success('添加成功', { description: result.data.message });
				// 重置表单
				sourceId = '';
				upId = '';
				name = '';
				path = '/Downloads';
				downloadAllSeasons = false;
				collectionType = 'season';
				isManualInput = false;
				bangumiSeasons = [];
				selectedSeasons = [];
				selectedVideos = [];
				selectedUpName = '';
				mergeToSourceId = null;
				existingBangumiSources = [];
				blacklistKeywords = [];
				whitelistKeywords = [];
				newBlacklistKeyword = '';
				newWhitelistKeyword = '';
				showKeywordSection = false;
				// 跳转到视频源管理页面
				goto('/video-sources');
			} else {
				toast.error('添加失败', { description: result.data.message });
			}
		} catch (error: unknown) {
			console.error('添加视频源失败:', error);

			// 解析错误信息，提供更友好的提示
			let errorMessage = error instanceof Error ? error.message : '添加视频源失败';
			let errorDescription = '';

			if (errorMessage.includes('已存在')) {
				// 重复添加错误
				if (sourceType === 'bangumi') {
					errorDescription =
						'该番剧已经添加过了，请检查是否使用了相同的Season ID、Media ID或Episode ID';
				} else if (sourceType === 'collection') {
					errorDescription = '该合集已经添加过了，请检查是否使用了相同的合集ID和UP主ID';
				} else if (sourceType === 'favorite') {
					errorDescription = '该收藏夹已经添加过了，请检查是否使用了相同的收藏夹ID';
				} else if (sourceType === 'submission') {
					errorDescription = '该UP主的投稿已经添加过了，请检查是否使用了相同的UP主ID';
				} else if (sourceType === 'watch_later') {
					errorDescription = '稍后观看只能配置一个，请先删除现有配置';
				}

				toast.error('重复添加', {
					description: errorDescription,
					duration: 5000 // 延长显示时间
				});
			} else {
				// 其他错误
				toast.error('添加失败', { description: errorMessage });
			}
		} finally {
			loading = false;
		}
	}

	// 根据类型显示不同的描述
	$: currentTypeDescription =
		sourceTypeOptions.find((opt) => opt.value === sourceType)?.description || '';

	// 获取收藏夹列表
	async function fetchUserFavorites() {
		loadingFavorites = true;
		try {
			const result = await api.getUserFavorites();
			if (result.data) {
				userFavorites = result.data;
				toast.success('获取收藏夹成功', {
					description: `共获取到 ${userFavorites.length} 个收藏夹`
				});
			} else {
				toast.error('获取收藏夹失败');
			}
		} catch (error: unknown) {
			console.error('获取收藏夹失败:', error);
			const errorMessage = error instanceof Error ? error.message : '获取收藏夹失败';
			toast.error('获取收藏夹失败', { description: errorMessage });
		} finally {
			loadingFavorites = false;
		}
	}

	// 选择收藏夹
	function selectFavorite(favorite: UserFavoriteFolder) {
		// 检查收藏夹是否已存在
		if (isFavoriteExists(favorite.id)) {
			toast.error('收藏夹已存在', {
				description: `该收藏夹「${favorite.name || favorite.title}」已经添加过了`
			});
			return;
		}

		sourceId = favorite.id.toString();
		name = favorite.name || favorite.title;
		favoriteValidationResult = {
			valid: true,
			fid: favorite.id,
			title: favorite.name || favorite.title,
			message: '收藏夹验证成功'
		};
		toast.success('已选择收藏夹', { description: name });
	}

	// 选择搜索到的收藏夹
	function selectSearchedFavorite(favorite: UserFavoriteFolder) {
		// 检查收藏夹是否已存在（使用完整ID）
		if (isFavoriteExists(favorite.id)) {
			toast.error('收藏夹已存在', {
				description: `该收藏夹「${favorite.title}」已经添加过了`
			});
			return;
		}

		// 使用完整ID（id字段），而不是短ID（fid字段）
		sourceId = favorite.id.toString();
		name = favorite.title;
		favoriteValidationResult = {
			valid: true,
			fid: Number(favorite.id),
			title: favorite.title,
			message: '收藏夹验证成功'
		};
		toast.success('已选择收藏夹', { description: name });
	}

	// 选择UP主并获取其收藏夹
	async function selectUserAndFetchFavorites(user: SearchResultItem) {
		selectedUserId = user.mid.toString();
		selectedUserName = user.title; // 使用搜索结果中的title

		loadingSearchedUserFavorites = true;
		searchedUserFavorites = [];

		// 关闭搜索结果
		showSearchResults = false;
		searchResults = [];
		searchKeyword = '';
		searchTotalResults = 0;

		try {
			const result = await api.getUserFavoritesByUid(selectedUserId);
			if (result.data && result.data.length > 0) {
				searchedUserFavorites = result.data;
				toast.success('获取收藏夹成功', {
					description: `从 ${selectedUserName} 获取到 ${searchedUserFavorites.length} 个收藏夹`
				});
			} else {
				toast.info('该UP主没有公开收藏夹');
			}
		} catch {
			console.error('获取UP主收藏夹失败');
			toast.error('获取收藏夹失败', {
				description: 'UP主可能没有公开收藏夹或网络错误'
			});
		} finally {
			loadingSearchedUserFavorites = false;
		}
	}

	// 验证收藏夹ID
	async function validateFavoriteId(fid: string) {
		if (!fid.trim()) {
			favoriteValidationResult = null;
			return;
		}

		// 检查是否为纯数字
		if (!/^\d+$/.test(fid.trim())) {
			favoriteValidationResult = {
				valid: false,
				fid: 0,
				title: '',
				message: '收藏夹ID必须为纯数字'
			};
			return;
		}

		validatingFavorite = true;
		favoriteValidationResult = null;

		try {
			const result = await api.validateFavorite(fid.trim());
			favoriteValidationResult = result.data;

			if (result.data.valid && !name) {
				// 如果验证成功且用户还没有填写名称，自动填入收藏夹标题
				name = result.data.title;
			}
		} catch {
			favoriteValidationResult = {
				valid: false,
				fid: parseInt(fid) || 0,
				title: '',
				message: '验证失败：网络错误或收藏夹不存在'
			};
		} finally {
			validatingFavorite = false;
		}
	}

	// 处理收藏夹ID变化
	function handleFavoriteIdChange() {
		clearTimeout(favoriteValidationTimeout);
		if (sourceType === 'favorite' && sourceId.trim()) {
			favoriteValidationTimeout = setTimeout(() => {
				validateFavoriteId(sourceId);
			}, 500);
		} else {
			favoriteValidationResult = null;
		}
	}

	// 处理UP主ID变化
	function handleUpIdChange() {
		clearTimeout(upIdTimeout);
		if (upId.trim()) {
			upIdTimeout = setTimeout(() => {
				fetchUserCollections();
			}, 500);
		} else {
			userCollections = [];
		}
	}

	// 获取UP主合集列表
	async function fetchUserCollections() {
		if (!upId.trim()) return;

		loadingCollections = true;
		try {
			const result = await api.getUserCollections(upId);
			if (result.data && result.data.collections) {
				userCollections = result.data.collections;
				if (userCollections.length === 0) {
					toast.info('该UP主暂无合集');
				} else {
					toast.success('获取合集列表成功', {
						description: `共获取到 ${userCollections.length} 个合集`
					});
				}
			} else {
				toast.error('获取合集列表失败');
				userCollections = [];
			}
		} catch (error: unknown) {
			console.error('获取合集列表失败:', error);

			// 根据错误类型提供更友好的提示
			let errorMessage = '获取合集列表失败';
			let errorDescription = '';

			const errorMsg = error instanceof Error ? error.message : '';

			if (errorMsg === 'Failed to fetch' || errorMsg.includes('ERR_EMPTY_RESPONSE')) {
				errorDescription = '该UP主的合集可能需要登录访问，或暂时无法获取';
			} else if (errorMsg.includes('403') || errorMsg.includes('Forbidden')) {
				errorDescription = '该UP主的合集为私有，无法访问';
			} else if (errorMsg.includes('404') || errorMsg.includes('Not Found')) {
				errorDescription = 'UP主不存在或合集已被删除';
			} else {
				errorDescription = '网络错误或服务暂时不可用，请稍后重试';
			}

			toast.error(errorMessage, { description: errorDescription });
			userCollections = [];
		} finally {
			loadingCollections = false;
		}
	}

	// 选择合集
	function selectCollection(collection: UserCollectionItem) {
		// 检查合集是否已存在
		if (isCollectionExists(collection.sid, collection.mid.toString())) {
			toast.error('合集已存在', {
				description: `该合集「${collection.name}」已经添加过了`
			});
			return;
		}

		sourceId = collection.sid;
		name = collection.name;
		cover = collection.cover || '';
		collectionType = collection.collection_type;
		isManualInput = false; // 从列表选择，不是手动输入
		toast.success('已选择合集', {
			description: `${collection.collection_type === 'season' ? '合集' : '系列'}：${collection.name}`
		});
	}

	// 处理Season ID变化
	// eslint-disable-next-line @typescript-eslint/no-unused-vars
	function handleSeasonIdChange() {
		clearTimeout(seasonIdTimeout);
		if (sourceId.trim() && sourceType === 'bangumi') {
			seasonIdTimeout = setTimeout(() => {
				fetchBangumiSeasons();
			}, 500);
		} else {
			bangumiSeasons = [];
			selectedSeasons = [];
		}
	}

	// 获取番剧季度信息
	async function fetchBangumiSeasons() {
		if (!sourceId.trim() || sourceType !== 'bangumi') return;

		loadingSeasons = true;
		try {
			const result = await api.getBangumiSeasons(sourceId);
			if (result.data && result.data.success) {
				bangumiSeasons = result.data.data || [];
				// 默认选中当前季度
				if (bangumiSeasons.length > 0) {
					const currentSeason = bangumiSeasons.find((s) => s.season_id === sourceId);
					if (currentSeason) {
						selectedSeasons = [currentSeason.season_id];
					}
				}
				// 如果只有一个季度，自动选中它
				if (bangumiSeasons.length === 1) {
					selectedSeasons = [bangumiSeasons[0].season_id];
				}
			} else {
				bangumiSeasons = [];
			}
		} catch (error: unknown) {
			console.error('获取季度信息失败:', error);
			const errorMessage = error instanceof Error ? error.message : '获取季度信息失败';
			toast.error('获取季度信息失败', { description: errorMessage });
			bangumiSeasons = [];
			selectedSeasons = [];
		} finally {
			loadingSeasons = false;
		}
	}

	// 获取现有番剧源列表（用于合并选择）
	async function fetchExistingBangumiSources() {
		loadingBangumiSources = true;
		try {
			const result = await api.getBangumiSourcesForMerge();
			if (result.data && result.data.success) {
				existingBangumiSources = result.data.bangumi_sources;
			} else {
				existingBangumiSources = [];
			}
		} catch (error) {
			console.error('获取现有番剧源失败:', error);
			existingBangumiSources = [];
		} finally {
			loadingBangumiSources = false;
		}
	}

	// 加载已有视频源（用于过滤）
	async function loadExistingVideoSources() {
		loadingExistingSources = true;
		try {
			const result = await api.getVideoSources();
			if (result.data) {
				existingVideoSources = result.data;

				// 处理合集：存储 s_id_m_id 的组合
				existingCollectionIds.clear();
				result.data.collection?.forEach((c) => {
					if (c.s_id && c.m_id) {
						const key = `${c.s_id}_${c.m_id}`;
						existingCollectionIds.add(key);
					}
				});

				// 处理收藏夹
				existingFavoriteIds.clear();
				result.data.favorite?.forEach((f) => {
					if (f.f_id) {
						existingFavoriteIds.add(f.f_id);
					}
				});

				// 处理UP主投稿
				existingSubmissionIds.clear();
				result.data.submission?.forEach((s) => {
					if (s.upper_id) {
						existingSubmissionIds.add(s.upper_id);
					}
				});

				// 处理番剧（主季度ID + 已选择的季度ID）
				existingBangumiSeasonIds.clear();
				result.data.bangumi?.forEach((b) => {
					if (b.season_id) {
						existingBangumiSeasonIds.add(b.season_id.toString());
					}
					// 如果有已选择的季度，也加入到过滤列表中
					if (b.selected_seasons) {
						try {
							// 检查 selected_seasons 是字符串还是已经解析的数组
							let selectedSeasons;
							if (typeof b.selected_seasons === 'string') {
								selectedSeasons = JSON.parse(b.selected_seasons);
							} else {
								selectedSeasons = b.selected_seasons;
							}

							if (Array.isArray(selectedSeasons)) {
								selectedSeasons.forEach((seasonId) => {
									// 确保统一转换为字符串进行比较
									const seasonIdStr = seasonId.toString();
									existingBangumiSeasonIds.add(seasonIdStr);
								});
							}
						} catch (e) {
							console.warn('解析selected_seasons失败:', b.selected_seasons, e);
						}
					}
				});
			}
		} catch (error) {
			console.error('加载已有视频源失败:', error);
		} finally {
			loadingExistingSources = false;
		}
	}

	// 检查合集是否已存在
	function isCollectionExists(sId: string, mId: string): boolean {
		const key = `${sId}_${mId}`;
		return existingCollectionIds.has(key);
	}

	// 检查UP主投稿是否已存在
	function isSubmissionExists(upperId: number): boolean {
		return existingSubmissionIds.has(upperId);
	}

	// 检查收藏夹是否已存在
	function isFavoriteExists(fId: number): boolean {
		return existingFavoriteIds.has(fId);
	}

	// 检查番剧季度是否已存在
	function isBangumiSeasonExists(seasonId: string): boolean {
		return existingBangumiSeasonIds.has(seasonId.toString());
	}

	// 切换季度选择
	function toggleSeasonSelection(seasonId: string) {
		// 检查季度是否已存在
		if (isBangumiSeasonExists(seasonId)) {
			const seasonName =
				filteredBangumiSeasons.find((s) => s.season_id === seasonId)?.season_title || '该季度';
			toast.error('季度已存在', {
				description: `${seasonName}已经添加过了`
			});
			return;
		}

		const index = selectedSeasons.indexOf(seasonId);
		if (index === -1) {
			selectedSeasons = [...selectedSeasons, seasonId];
		} else {
			selectedSeasons = selectedSeasons.filter((id) => id !== seasonId);
		}
	}

	// 过滤后的收藏夹列表（不完全过滤，而是标记已存在状态）
	$: filteredUserFavorites = userFavorites;

	$: filteredSearchedUserFavorites = searchedUserFavorites;

	// 过滤后的合集列表（不完全过滤，而是标记已存在状态）
	$: filteredUserCollections = userCollections;

	// 过滤后的关注UP主列表（不完全过滤，而是标记已存在状态）
	$: filteredUserFollowings = userFollowings;

	// 过滤后的搜索结果（根据类型过滤已存在的源）
	$: filteredSearchResults = searchResults.filter((result) => {
		if (sourceType === 'submission' && result.mid) {
			return !existingSubmissionIds.has(Number(result.mid));
		}
		// 对于番剧和合集搜索，不完全过滤，显示所有结果但标记已存在状态
		return true;
	});

	// 过滤后的番剧季度列表（标记已存在的季度）
	$: filteredBangumiSeasons = bangumiSeasons.map((season) => ({
		...season,
		isExisting: isBangumiSeasonExists(season.season_id)
	}));

	// 监听sourceType变化，清理季度相关状态
	$: if (sourceType !== 'bangumi') {
		bangumiSeasons = [];
		selectedSeasons = [];
		showMergeOptions = false;
		mergeToSourceId = null;
	}

	// 当源类型改为番剧时，获取现有番剧源列表
	$: if (sourceType === 'bangumi') {
		fetchExistingBangumiSources();
	}

	// 监听sourceType变化，重置手动输入标志和清空所有缓存
	$: if (sourceType) {
		isManualInput = false;
		// 清空搜索相关状态
		searchResults = [];
		searchKeyword = '';
		searchTotalResults = 0;
		showSearchResults = false;
		hoveredItem = null;
		// 清空各类型的缓存数据
		userFollowings = [];
		userCollections = [];
		userFavorites = [];
		subscribedCollections = [];
		// 清空UP主收藏夹搜索状态
		searchedUserFavorites = [];
		selectedUserId = '';
		selectedUserName = '';
		loadingSearchedUserFavorites = false;
		// 注意：bangumiSeasons 和 selectedSeasons 在另一个响应式语句中处理
	}

	// 监听 source_id 变化，自动获取季度信息
	$: if (sourceType === 'bangumi' && sourceId) {
		fetchBangumiSeasons();
	}

	// 切换源类型时，如处于批量模式且已有选择，则清空选择防止跨源类型
	$: if (sourceType !== lastSourceType) {
		if (batchMode && batchSelectedItems.size > 0) {
			clearBatchSelection();
			toast('已切换源类型，已清空批量选择，请重新选择');
		}
		lastSourceType = sourceType;
	}

	// 统一的悬浮处理函数
	function handleItemMouseEnter(
		type: 'search' | 'season',
		data: SearchResultItem | BangumiSeasonInfo,
		event: MouseEvent
	) {
		hoveredItem = { type, data };
		updateTooltipPosition(event);
	}

	function handleItemMouseMove(event: MouseEvent) {
		if (hoveredItem) {
			updateTooltipPosition(event);
		}
	}

	function updateTooltipPosition(event: MouseEvent) {
		// 获取视窗尺寸
		const viewportWidth = window.innerWidth;
		const viewportHeight = window.innerHeight;
		const tooltipWidth = 400; // 预估悬浮窗宽度
		const tooltipHeight = 300; // 预估悬浮窗高度

		let x = event.pageX + 20;
		let y = event.pageY - 100;

		// 防止悬浮窗超出右边界
		if (x + tooltipWidth > viewportWidth) {
			x = event.pageX - tooltipWidth - 20;
		}

		// 防止悬浮窗超出下边界
		if (y + tooltipHeight > viewportHeight) {
			y = event.pageY - tooltipHeight - 20;
		}

		// 防止悬浮窗超出上边界和左边界
		mousePosition = {
			x: Math.max(10, x),
			y: Math.max(10, y)
		};
	}

	function handleItemMouseLeave() {
		hoveredItem = null;
	}

	// 为了向后兼容，保留旧的函数名但重定向到新的统一函数
	function handleMouseEnter(result: SearchResultItem, event: MouseEvent) {
		handleItemMouseEnter('search', result, event);
	}

	function handleMouseMove(event: MouseEvent) {
		handleItemMouseMove(event);
	}

	function handleMouseLeave() {
		handleItemMouseLeave();
	}

	function handleSeasonMouseEnter(season: BangumiSeasonInfo, event: MouseEvent) {
		handleItemMouseEnter('season', season, event);
	}

	function handleSeasonMouseMove(event: MouseEvent) {
		handleItemMouseMove(event);
	}

	function handleSeasonMouseLeave() {
		handleItemMouseLeave();
	}

	// 获取关注的UP主列表
	async function fetchUserFollowings() {
		loadingFollowings = true;
		try {
			const result = await api.getUserFollowings();
			if (result.data) {
				userFollowings = result.data;
				toast.success('获取关注UP主成功', {
					description: `共获取到 ${userFollowings.length} 个UP主`
				});
			} else {
				toast.error('获取关注UP主失败');
			}
		} catch (error: unknown) {
			console.error('获取关注UP主失败:', error);
			const errorMessage = error instanceof Error ? error.message : '获取关注UP主失败';
			toast.error('获取关注UP主失败', { description: errorMessage });
		} finally {
			loadingFollowings = false;
		}
	}

	// 选择关注的UP主
	function selectFollowing(following: UserFollowing) {
		switch (sourceType) {
			case 'collection':
				upId = following.mid.toString();
				// 触发获取UP主合集列表
				handleUpIdChange();
				toast.success('已填充UP主信息', { description: '正在获取合集列表...' });
				break;
			case 'submission':
				sourceId = following.mid.toString();
				name = following.name;
				selectedUpName = following.name;
				// 打开投稿选择对话框
				showSubmissionSelection = true;
				toast.success('已填充UP主信息');
				break;
		}

		// 清空关注UP主列表状态，关闭面板
		userFollowings = [];
	}

	// 获取关注的收藏夹列表
	async function fetchSubscribedCollections() {
		loadingSubscribedCollections = true;
		try {
			const result = await api.getSubscribedCollections();
			if (result.data) {
				subscribedCollections = result.data;
				if (subscribedCollections.length === 0) {
					toast.info('暂无关注的合集', {
						description: '您还没有关注任何合集。关注合集后可以在这里快速选择添加。',
						duration: 5000
					});
				} else {
					toast.success('获取关注的合集成功', {
						description: `共获取到 ${subscribedCollections.length} 个您关注的合集`
					});
				}
			} else {
				toast.error('获取合集失败');
			}
		} catch (error: unknown) {
			console.error('获取合集失败:', error);
			const errorMessage = error instanceof Error ? error.message : '获取合集失败';
			toast.error('获取合集失败', { description: errorMessage });
		} finally {
			loadingSubscribedCollections = false;
		}
	}

	// 选择订阅的合集或收藏夹
	function selectSubscribedCollection(collection: UserCollectionInfo) {
		// 根据 collection_type 决定添加为收藏夹还是合集
		if (collection.collection_type === 'favorite') {
			// 这是收藏夹，切换到收藏夹模式
			sourceType = 'favorite';
			sourceId = collection.sid;
			name = collection.name;
			// 收藏夹不需要 upId 和 cover
			upId = '';
			cover = '';
			collectionType = '';
			toast.success('已选择收藏夹', { description: collection.name });
		} else {
			// 这是合集，保持合集模式
			sourceType = 'collection';
			sourceId = collection.sid;
			name = collection.name;
			cover = collection.cover || '';
			upId = collection.up_mid.toString();
			collectionType = collection.collection_type;
			toast.success('已选择合集', { description: collection.name });
		}
	}

	// 处理投稿选择确认
	// eslint-disable-next-line @typescript-eslint/no-unused-vars
	function handleSubmissionSelectionConfirm(selectedBvids: string[]) {
		selectedVideos = selectedBvids;
		showSubmissionSelection = false;
		if (selectedBvids.length > 0) {
			toast.success('已选择投稿', {
				description: `选择了 ${selectedBvids.length} 个历史投稿，新投稿将自动下载`
			});
		} else {
			toast.info('未选择投稿', {
				description: '将下载所有历史投稿和新投稿'
			});
		}
	}

	// 处理投稿选择取消
	// eslint-disable-next-line @typescript-eslint/no-unused-vars
	function handleSubmissionSelectionCancel() {
		showSubmissionSelection = false;
		// 保留已有的选择，不做清空
	}

	// 投稿选择相关函数

	// 重置投稿选择状态
	function resetSubmissionState() {
		submissionVideos = [];
		selectedSubmissionVideos = new Set();
		submissionLoading = false;
		submissionError = null;
		submissionTotalCount = 0;
		submissionSearchQuery = '';
		filteredSubmissionVideos = [];
	}

	// 搜索相关状态
	let searchTimeout: NodeJS.Timeout;
	let isSearching = false;

	// 搜索过滤投稿 - 使用后端API搜索
	// eslint-disable-next-line svelte/infinite-reactive-loop
	$: {
		if (submissionSearchQuery.trim()) {
			// 清除之前的搜索定时器
			if (searchTimeout) {
				clearTimeout(searchTimeout);
			}

			// 设置新的搜索定时器（防抖）
			searchTimeout = setTimeout(() => {
				// eslint-disable-next-line svelte/infinite-reactive-loop
				performSearch();
			}, 500); // 500ms防抖
		} else {
			filteredSubmissionVideos = submissionVideos;
		}
	}

	// 执行搜索
	/* eslint-disable svelte/infinite-reactive-loop */
	async function performSearch() {
		if (!sourceId || !submissionSearchQuery.trim()) {
			filteredSubmissionVideos = submissionVideos;
			return;
		}

		isSearching = true;

		try {
			const response = await api.getSubmissionVideos({
				up_id: sourceId,
				page: 1,
				page_size: 30, // 获取更多结果
				keyword: submissionSearchQuery.trim()
			});

			if (response.data && response.data.videos) {
				filteredSubmissionVideos = response.data.videos;
			} else {
				filteredSubmissionVideos = [];
			}
		} catch (error) {
			console.error('搜索视频失败:', error);
			toast.error('搜索失败', {
				description: '请稍后重试'
			});
			// 搜索失败时回退到本地过滤
			filteredSubmissionVideos = submissionVideos.filter((video) =>
				video.title.toLowerCase().includes(submissionSearchQuery.toLowerCase().trim())
			);
		} finally {
			isSearching = false;
		}
	}
	/* eslint-enable svelte/infinite-reactive-loop */

	// 加载UP主投稿列表（分页加载，初始100个）
	async function loadSubmissionVideos() {
		if (!sourceId) return;

		submissionLoading = true;
		submissionError = null;
		submissionVideos = [];
		currentLoadedPage = 0;
		hasMoreVideos = true;
		showLoadMoreButton = false;

		try {
			await loadVideosInBatch(INITIAL_LOAD_SIZE);
		} catch (err) {
			submissionError = err instanceof Error ? err.message : '网络请求失败';
		} finally {
			submissionLoading = false;
		}
	}

	// 批量加载视频（串行请求，带延迟）
	async function loadVideosInBatch(loadCount: number) {
		const startPage = currentLoadedPage + 1;
		const targetVideos = Math.min(
			submissionVideos.length + loadCount,
			submissionTotalCount || Infinity
		);
		const neededPages = Math.ceil(targetVideos / SUBMISSION_PAGE_SIZE);

		for (let page = startPage; page <= neededPages; page++) {
			// 更新进度
			loadingProgress = `正在加载第 ${page} 页...`;

			// 延迟（除了第一页）
			if (page > startPage) {
				await new Promise((resolve) => setTimeout(resolve, PAGE_DELAY));
			}

			const response = await api.getSubmissionVideos({
				up_id: sourceId,
				page: page,
				page_size: SUBMISSION_PAGE_SIZE
			});

			if (!response.data) {
				throw new Error('获取投稿列表失败');
			}

			// 第一次请求时获取总数
			if (page === 1 && submissionTotalCount === 0) {
				submissionTotalCount = response.data.total;
			}

			// 添加新视频（去重）
			const newVideos = response.data.videos || [];
			const existingBvids = new Set(submissionVideos.map((v) => v.bvid));
			const uniqueNewVideos = newVideos.filter((video) => !existingBvids.has(video.bvid));

			submissionVideos = [...submissionVideos, ...uniqueNewVideos];
			currentLoadedPage = page;

			// 检查是否达到目标数量或已加载全部
			if (
				submissionVideos.length >= targetVideos ||
				submissionVideos.length >= submissionTotalCount
			) {
				break;
			}
		}

		// 更新状态
		hasMoreVideos = submissionVideos.length < submissionTotalCount;
		// 不自动显示按钮，等待用户滚动到底部时才显示
		loadingProgress = '';
	}

	// 加载更多投稿视频
	async function loadMoreSubmissionVideos() {
		if (!hasMoreVideos || isLoadingMore) return;

		isLoadingMore = true;
		showLoadMoreButton = false; // 隐藏按钮
		try {
			await loadVideosInBatch(LOAD_MORE_SIZE);
		} catch (err) {
			console.error('加载更多视频失败:', err);
			toast.error('加载更多视频失败', {
				description: err instanceof Error ? err.message : '网络请求失败'
			});
		} finally {
			isLoadingMore = false;
		}
	}

	// 处理滚动事件，检测是否需要显示加载更多按钮
	function handleSubmissionScroll(event: Event) {
		const container = event.target as HTMLElement;
		if (!container || !hasMoreVideos) return;

		const { scrollTop, scrollHeight, clientHeight } = container;
		const threshold = 100; // 距离底部100px时显示按钮

		// 当滚动接近底部时显示加载更多按钮
		if (scrollHeight - scrollTop - clientHeight < threshold) {
			showLoadMoreButton = true;
		}
	}

	// 处理视频选择
	function toggleSubmissionVideo(bvid: string) {
		if (selectedSubmissionVideos.has(bvid)) {
			selectedSubmissionVideos.delete(bvid);
		} else {
			selectedSubmissionVideos.add(bvid);
		}
		selectedSubmissionVideos = selectedSubmissionVideos; // 触发响应式更新
	}

	// 全选投稿
	function selectAllSubmissions() {
		filteredSubmissionVideos.forEach((video) => selectedSubmissionVideos.add(video.bvid));
		selectedSubmissionVideos = selectedSubmissionVideos;
	}

	// 全不选投稿
	function selectNoneSubmissions() {
		filteredSubmissionVideos.forEach((video) => selectedSubmissionVideos.delete(video.bvid));
		selectedSubmissionVideos = selectedSubmissionVideos;
	}

	// 反选投稿
	function invertSubmissionSelection() {
		filteredSubmissionVideos.forEach((video) => {
			if (selectedSubmissionVideos.has(video.bvid)) {
				selectedSubmissionVideos.delete(video.bvid);
			} else {
				selectedSubmissionVideos.add(video.bvid);
			}
		});
		selectedSubmissionVideos = selectedSubmissionVideos;
	}

	// 确认投稿选择
	function confirmSubmissionSelection() {
		selectedVideos = Array.from(selectedSubmissionVideos);
		showSubmissionSelection = false;
		if (selectedVideos.length > 0) {
			toast.success('已选择投稿', {
				description: `选择了 ${selectedVideos.length} 个历史投稿，新投稿将自动下载`
			});
		} else {
			toast.info('未选择投稿', {
				description: '将下载所有历史投稿和新投稿'
			});
		}
	}

	// 取消投稿选择
	function cancelSubmissionSelection() {
		showSubmissionSelection = false;
		// 保留已有的选择，不做清空
	}

	// 格式化时间
	function formatSubmissionDate(pubtime: string): string {
		try {
			return new Date(pubtime).toLocaleDateString('zh-CN');
		} catch {
			return pubtime;
		}
	}

	// 格式化播放量
	function formatSubmissionPlayCount(count: number): string {
		if (count >= 10000) {
			return (count / 10000).toFixed(1) + '万';
		}
		return count.toString();
	}

	// 当显示投稿选择且有sourceId时加载数据
	$: if (showSubmissionSelection && sourceId && sourceType === 'submission') {
		resetSubmissionState();
		loadSubmissionVideos();
	}

	// 计算已选择的投稿数量
	$: selectedSubmissionCount = Array.from(selectedSubmissionVideos).filter((bvid) =>
		filteredSubmissionVideos.some((video) => video.bvid === bvid)
	).length;

	// 批量选择相关函数
	function resolveBatchItemSourceTypeByRawType(itemType: string): string {
		// 将原始的批量项类别映射为其对应的视频源类型
		switch (itemType) {
			case 'search':
			case 'following':
				return sourceType; // 跟随当前选择的源类型
			case 'favorite':
				return 'favorite';
			case 'collection':
				return 'collection';
			case 'bangumi':
				return 'bangumi';
			default:
				return sourceType;
		}
	}
	function toggleBatchSelection(itemKey: string, item: any, itemType: string) {
		console.log('🔵 toggleBatchSelection called with:', {
			itemKey,
			itemType,
			batchMode,
			currentMapSize: batchSelectedItems.size
		});

		if (batchSelectedItems.has(itemKey)) {
			console.log('🔴 Removing item:', itemKey);
			batchSelectedItems.delete(itemKey);
		} else {
			console.log('🟢 Adding item:', itemKey);

			// 先做跨源类型限制：不允许在一次批量中混合不同视频源类型
			const newItemSourceType = resolveBatchItemSourceTypeByRawType(itemType);
			if (batchSelectedItems.size > 0) {
				const first = batchSelectedItems.values().next().value;
				const currentBatchSourceType = getSourceTypeFromBatchItem(first);
				if (newItemSourceType !== currentBatchSourceType) {
					console.log('❌ Cross-type selection rejected');
					toast.error('批量模式不支持跨源类型选择', {
						description: '请先清空已选项，再选择其他源类型的内容'
					});
					return;
				}
			}

			// 生成默认名称
			let itemName = '';

			switch (itemType) {
				case 'search':
					itemName = cleanTitle(item.title);
					break;
				case 'favorite':
					itemName = item.name || item.title;
					break;
				case 'collection':
					itemName = item.name || item.title;
					break;
				case 'following':
					itemName = item.name;
					break;
				case 'bangumi':
					itemName = item.season_title || item.title || item.full_title;
					break;
			}

			batchSelectedItems.set(itemKey, {
				type: itemType,
				data: item,
				name: itemName
			});
		}

		console.log('📊 After operation:', {
			newMapSize: batchSelectedItems.size,
			hasItem: batchSelectedItems.has(itemKey),
			allKeys: Array.from(batchSelectedItems.keys())
		});

		batchSelectedItems = new Map(batchSelectedItems);
		console.log('✅ New Map created for reactivity');
	}

	function isBatchSelected(itemKey: string): boolean {
		return !!batchCheckboxStates[itemKey];
	}

	function clearBatchSelection() {
		console.log('🧹 Clearing batch selection');
		batchSelectedItems = new Map();
		console.log('✅ New empty Map created');
	}

	function selectAllVisible(itemType: string) {
		switch (itemType) {
			case 'search':
				filteredSearchResults.forEach((result, index) => {
					const key = `search_${result.bvid || result.season_id || result.mid || index}`;
					if (!batchSelectedItems.has(key)) {
						toggleBatchSelection(key, result, 'search');
					}
				});
				break;
			case 'following':
				filteredUserFollowings.forEach((following) => {
					const key = `following_${following.mid}`;
					// 跳过已添加的UP主
					const isDisabled =
						sourceType === 'submission' && existingSubmissionIds.has(following.mid);
					if (!batchSelectedItems.has(key) && !isDisabled) {
						toggleBatchSelection(key, following, 'following');
					}
				});
				break;
			case 'favorite':
				userFavorites.forEach((favorite) => {
					const key = `favorite_${favorite.id}`;
					// 跳过已添加的收藏夹
					const isDisabled = existingFavoriteIds.has(favorite.id);
					if (!batchSelectedItems.has(key) && !isDisabled) {
						toggleBatchSelection(key, favorite, 'favorite');
					}
				});
				break;
			case 'searched-favorite':
				searchedUserFavorites.forEach((favorite) => {
					const key = `searched-favorite_${favorite.id}`;
					// 跳过已添加的收藏夹（使用完整ID）
					const isDisabled = existingFavoriteIds.has(Number(favorite.id));
					if (!batchSelectedItems.has(key) && !isDisabled) {
						toggleBatchSelection(key, favorite, 'favorite');
					}
				});
				break;
			case 'collection':
				userCollections.forEach((collection) => {
					const key = `collection_${collection.sid}`;
					// 跳过已添加的合集
					const isDisabled = isCollectionExists(collection.sid, collection.mid.toString());
					if (!batchSelectedItems.has(key) && !isDisabled) {
						toggleBatchSelection(key, collection, 'collection');
					}
				});
				break;
			case 'subscribed-collection':
				subscribedCollections.forEach((collection) => {
					const key = `subscribed-collection_${collection.sid}`;
					// 跳过已添加的合集
					const isDisabled = isCollectionExists(collection.sid, collection.up_mid.toString());
					if (!batchSelectedItems.has(key) && !isDisabled) {
						toggleBatchSelection(key, collection, 'collection');
					}
				});
				break;
		}
	}

	// 批量添加函数
	async function handleBatchAdd() {
		if (batchSelectedItems.size === 0) {
			toast.error('未选择任何视频源');
			return;
		}

		// 校验所有被选项是否属于同一视频源类型，防止跨源类型批量添加
		const resolvedTypes = new Set(
			Array.from(batchSelectedItems.values()).map((it: any) => getSourceTypeFromBatchItem(it))
		);
		if (resolvedTypes.size > 1) {
			toast.error('不能跨源类型批量添加', {
				description: '已检测到多种源类型，请清空后按类型分别添加'
			});
			return;
		}
		batchAdding = true;
		batchProgress = { current: 0, total: batchSelectedItems.size };

		const selectedItems = Array.from(batchSelectedItems.entries());
		let successCount = 0;
		let failedCount = 0;
		const failedItems: string[] = [];

		try {
			for (let i = 0; i < selectedItems.length; i++) {
				const [itemKey, item] = selectedItems[i];
				batchProgress.current = i + 1;

				try {
					// 构建添加视频源的参数
					const params: AddVideoSourceRequest = {
						source_type: getSourceTypeFromBatchItem(item),
						source_id: getSourceIdFromBatchItem(item),
						name: item.name,
						path: batchBasePath
					};

					// 添加特定类型的额外参数
					if (item.type === 'following') {
						if (sourceType === 'collection') {
							params.up_id = item.data.mid.toString();
							params.collection_type = 'season';
						} else if (sourceType === 'submission') {
							// 批量添加UP主投稿时总是使用全部投稿模式
						}
					} else if (item.type === 'collection') {
						// 区分普通合集和关注的合集
						if (itemKey.startsWith('subscribed-collection_')) {
							// 关注的合集使用 up_mid
							params.up_id = item.data.up_mid.toString();
							params.collection_type = item.data.collection_type || 'season';
						} else {
							// 普通合集使用 mid
							params.up_id = item.data.mid.toString();
							params.collection_type = item.data.type || 'season';
						}
					}

					if (item.data.cover) {
						params.cover = item.data.cover;
					}

					const result = await api.addVideoSource(params);

					if (result.data.success) {
						successCount++;
					} else {
						failedCount++;
						failedItems.push(item.name);
					}
				} catch (error) {
					failedCount++;
					failedItems.push(item.name);
					console.error(`添加视频源 ${item.name} 失败:`, error);
				}

				// 添加小延迟避免请求过于频繁
				await new Promise((resolve) => setTimeout(resolve, 200));
			}

			// 显示结果
			if (successCount > 0 && failedCount === 0) {
				toast.success('批量添加完成', {
					description: `成功添加 ${successCount} 个视频源`
				});
			} else if (successCount > 0) {
				toast.success('批量添加部分成功', {
					description: `成功添加 ${successCount} 个，失败 ${failedCount} 个`
				});
			} else {
				toast.error('批量添加失败', {
					description: '所有视频源都添加失败'
				});
			}

			// 清空选择并关闭批量模式
			clearBatchSelection();
			batchMode = false;
			batchDialogOpen = false;

			// 如果有成功添加的，跳转到视频源管理页面
			if (successCount > 0) {
				setTimeout(() => {
					goto('/video-sources');
				}, 1000);
			}
		} catch (error) {
			console.error('批量添加过程中发生错误:', error);
			toast.error('批量添加失败', {
				description: '添加过程中发生错误'
			});
		} finally {
			batchAdding = false;
			batchProgress = { current: 0, total: 0 };
		}
	}

	// 根据批量选择项获取视频源类型
	function getSourceTypeFromBatchItem(item: any): string {
		switch (item.type) {
			case 'search':
				return sourceType; // 使用当前选择的源类型
			case 'following':
				return sourceType; // 使用当前选择的源类型（submission 或 collection）
			case 'favorite':
				return 'favorite';
			case 'collection':
				return 'collection';
			case 'bangumi':
				return 'bangumi';
			default:
				return sourceType;
		}
	}

	// 根据批量选择项获取视频源ID
	function getSourceIdFromBatchItem(item: any): string {
		switch (item.type) {
			case 'search':
				if (sourceType === 'submission') {
					return item.data.mid?.toString() || '';
				}
				return item.data.bvid || item.data.season_id || item.data.mid?.toString() || '';
			case 'following':
				return item.data.mid.toString();
			case 'favorite':
				// 处理两种收藏夹数据结构：用户自己的收藏夹使用id，搜索到的收藏夹使用fid
				return (item.data.fid || item.data.id).toString();
			case 'collection':
				return item.data.sid.toString();
			case 'bangumi':
				return item.data.season_id || '';
			default:
				return '';
		}
	}
</script>

<svelte:head>
	<title>添加视频源 - Bili Sync</title>
</svelte:head>

<svelte:window bind:innerWidth />

<div class="py-2">
	<div class="mx-auto px-4">
		<div class="bg-card rounded-lg border p-6 shadow-sm">
			<div class="mb-6 flex items-center justify-between">
				<h1 class="text-2xl font-bold">添加新视频源</h1>
				{#if sourceType !== 'bangumi' && sourceType !== 'watch_later'}
					<Button
						variant={batchMode ? 'default' : 'outline'}
						size="sm"
						onclick={() => {
							batchMode = !batchMode;
							if (!batchMode) {
								batchSelectedItems.clear();
								batchSelectedItems = batchSelectedItems;
							}
						}}
						class="flex items-center gap-2"
					>
						{#if batchMode}
							<X class="h-4 w-4" />
							退出批量模式
						{:else}
							<PlusIcon class="h-4 w-4" />
							批量添加
						{/if}
					</Button>
				{/if}
			</div>

			<div class="flex {isMobile ? 'flex-col' : 'gap-8'}">
				<!-- 左侧：表单区域 -->
				<div class={isMobile ? 'w-full' : 'max-w-[500px] min-w-[350px] flex-1'}>
					<form
						onsubmit={(e) => {
							e.preventDefault();
							handleSubmit();
						}}
						class="space-y-6"
					>
						<!-- 视频源类型 -->
						<div class="space-y-2">
							<Label for="source-type">视频源类型</Label>
							<select
								id="source-type"
								bind:value={sourceType}
								class="border-input bg-background ring-offset-background focus-visible:ring-ring flex h-10 w-full rounded-md border px-3 py-2 text-sm focus-visible:ring-2 focus-visible:ring-offset-2 focus-visible:outline-none"
							>
								{#each sourceTypeOptions as option (option.value)}
									<option value={option.value}>{option.label}</option>
								{/each}
							</select>
							<p class="text-muted-foreground text-sm">{currentTypeDescription}</p>
						</div>

						<!-- 搜索功能 -->
						{#if sourceType !== 'favorite' && sourceType !== 'watch_later'}
							<div
								class="rounded-lg border border-blue-200 bg-blue-50 p-4 dark:border-blue-800 dark:bg-blue-950"
							>
								<div class="space-y-2">
									<div>
										<Label for="search">
											{#if sourceType === 'collection'}
												搜索UP主
											{:else if sourceType === 'submission'}
												搜索UP主
											{:else if sourceType === 'bangumi'}
												搜索番剧
											{:else}
												搜索B站内容
											{/if}
										</Label>
										<div class="flex {isMobile ? 'flex-col gap-2' : 'gap-2'} mt-2">
											<Input
												id="search"
												bind:value={searchKeyword}
												placeholder={sourceType === 'submission' || sourceType === 'collection'
													? '搜索UP主...'
													: sourceType === 'bangumi'
														? '搜索番剧...'
														: '搜索视频...'}
												onkeydown={(e) => e.key === 'Enter' && handleSearch()}
											/>
											<div class="flex gap-2">
												<Button
													onclick={() => handleSearch()}
													disabled={searchLoading || !searchKeyword.trim()}
													size="sm"
													class={isMobile ? 'flex-1' : ''}
												>
													{#if searchLoading}
														搜索中...
													{:else}
														<Search class="h-4 w-4" />
													{/if}
												</Button>
												{#if sourceType === 'collection' || sourceType === 'submission'}
													<Button
														onclick={sourceType === 'collection'
															? fetchSubscribedCollections
															: fetchUserFollowings}
														disabled={sourceType === 'collection'
															? loadingSubscribedCollections
															: loadingFollowings}
														size="sm"
														variant="outline"
														class={isMobile ? 'flex-1' : ''}
													>
														{sourceType === 'collection'
															? loadingSubscribedCollections
																? '获取中...'
																: '获取关注的合集'
															: loadingFollowings
																? '获取中...'
																: '获取关注'}
													</Button>
												{/if}
											</div>
										</div>
										<p class="text-muted-foreground mt-1 text-xs">
											{#if sourceType === 'collection'}
												搜索UP主后会自动填充UP主ID，并显示该UP主的所有合集供选择
											{:else if sourceType === 'submission'}
												搜索并选择UP主，将自动填充UP主ID
											{:else if sourceType === 'bangumi'}
												搜索并选择番剧，将自动填充Season ID
											{:else}
												根据当前选择的视频源类型搜索对应内容
											{/if}
										</p>
									</div>
								</div>
							</div>
						{/if}

						<!-- 收藏夹列表（仅收藏夹类型时显示） -->
						{#if sourceType === 'favorite'}
							<div class="space-y-4">
								<!-- 我的收藏夹 -->
								<div
									class="rounded-lg border border-yellow-200 bg-yellow-50 p-4 dark:border-yellow-800 dark:bg-yellow-950"
								>
									<div
										class="flex {isMobile ? 'flex-col gap-2' : 'items-center justify-between'} mb-2"
									>
										<span class="text-sm font-medium text-yellow-800 dark:text-yellow-200"
											>我的收藏夹</span
										>
										<Button
											size="sm"
											variant="outline"
											onclick={fetchUserFavorites}
											disabled={loadingFavorites}
											class={isMobile ? 'w-full' : ''}
										>
											{loadingFavorites ? '加载中...' : '获取收藏夹'}
										</Button>
									</div>

									{#if userFavorites.length > 0}
										<p class="text-xs text-yellow-600 dark:text-yellow-400">
											已获取 {userFavorites.length} 个收藏夹，请在{isMobile ? '下方' : '右侧'}选择
										</p>
									{:else}
										<p class="text-xs text-yellow-600 dark:text-yellow-400">
											点击右侧按钮获取您的收藏夹列表
										</p>
									{/if}
								</div>

								<!-- 他人的公开收藏夹 -->
								<div
									class="rounded-lg border border-blue-200 bg-blue-50 p-4 dark:border-blue-800 dark:bg-blue-950"
								>
									<div class="mb-3">
										<span class="text-sm font-medium text-blue-800 dark:text-blue-200"
											>他人的公开收藏夹</span
										>
									</div>

									<!-- 搜索UP主的收藏夹 -->
									<div class="bg-card mb-4 rounded border border-gray-200 p-3">
										<div class="mb-2">
											<Label class="text-foreground text-sm font-medium">搜索UP主的收藏夹</Label>
										</div>
										<div class="flex {isMobile ? 'flex-col gap-2' : 'gap-2'}">
											<Input
												placeholder="搜索UP主名称..."
												bind:value={searchKeyword}
												onkeydown={(e) => e.key === 'Enter' && handleSearch()}
											/>
											<Button
												onclick={() => handleSearch()}
												disabled={searchLoading || !searchKeyword.trim()}
												size="sm"
												class={isMobile ? 'w-full' : ''}
											>
												{#if searchLoading}搜索中...{:else}搜索{/if}
											</Button>
										</div>

										<p class="text-muted-foreground mt-2 text-xs">
											{#if showSearchResults && searchResults.length > 0}
												找到 {searchResults.length} 个UP主，请在{isMobile
													? '下方'
													: '右侧'}列表中选择
											{:else}
												输入UP主名称后点击搜索，结果将在{isMobile ? '下方' : '右侧'}显示
											{/if}
										</p>
									</div>

									<!-- 手动输入收藏夹ID -->
									<div class="text-xs text-blue-600 dark:text-blue-400">
										<strong>或者手动输入收藏夹ID：</strong><br />
										1. 打开想要添加的收藏夹页面<br />
										2. 复制URL中 "fid=" 后面的数字<br />
										3. 在下方输入框中填写该数字
									</div>
								</div>
							</div>
						{/if}

						<!-- 合集类型（仅合集时显示，且手动输入） -->
						{#if sourceType === 'collection' && isManualInput}
							<div class="space-y-2">
								<Label for="collection-type">合集类型</Label>
								<select
									id="collection-type"
									bind:value={collectionType}
									class="border-input bg-background ring-offset-background focus-visible:ring-ring flex h-10 w-full rounded-md border px-3 py-2 text-sm focus-visible:ring-2 focus-visible:ring-offset-2 focus-visible:outline-none"
								>
									{#each collectionTypeOptions as option (option.value)}
										<option value={option.value}>{option.label}</option>
									{/each}
								</select>
								<p class="text-sm text-orange-600">
									⚠️ 手动输入合集ID时需要指定类型，建议从{isMobile ? '下方' : '右侧'}合集列表中选择
								</p>
							</div>
						{/if}

						<!-- UP主ID（仅合集时显示） -->
						{#if sourceType === 'collection'}
							<div class="space-y-2">
								<Label for="up-id">UP主ID</Label>
								<Input
									id="up-id"
									bind:value={upId}
									placeholder="请输入UP主ID"
									onblur={handleUpIdChange}
									required
								/>
								{#if userCollections.length > 0}
									<p class="mt-1 text-xs text-green-600">
										✓ 已获取合集列表，请在{isMobile ? '下方' : '右侧'}选择
									</p>
								{/if}
							</div>
						{/if}

						<!-- 视频源ID（稍后观看除外） -->
						{#if sourceType !== 'watch_later'}
							<div class="space-y-2">
								<Label for="source-id">
									{#if sourceType === 'collection'}合集ID
									{:else if sourceType === 'favorite'}收藏夹ID
									{:else if sourceType === 'submission'}UP主ID
									{:else if sourceType === 'bangumi'}Season ID
									{:else}ID{/if}
								</Label>
								<Input
									id="source-id"
									bind:value={sourceId}
									placeholder={`请输入${sourceType === 'collection' ? '合集' : sourceType === 'favorite' ? '任意公开收藏夹' : sourceType === 'submission' ? 'UP主' : sourceType === 'bangumi' ? 'Season' : ''}ID`}
									oninput={() => {
										if (sourceType === 'collection') {
											isManualInput = true;
										} else if (sourceType === 'favorite') {
											handleFavoriteIdChange();
										}
									}}
									required
								/>
								{#if sourceType === 'collection' && !isManualInput && sourceId}
									<p class="mt-1 text-xs text-green-600">✓ 已从列表中选择合集，类型已自动识别</p>
								{/if}
								{#if sourceType === 'favorite' && sourceId}
									{#if validatingFavorite}
										<p class="mt-1 text-xs text-blue-600 dark:text-blue-400">🔍 验证收藏夹中...</p>
									{:else if favoriteValidationResult}
										{#if favoriteValidationResult.valid}
											<p class="mt-1 text-xs text-green-600">
												✓ 收藏夹验证成功：{favoriteValidationResult.title}
											</p>
										{:else}
											<p class="mt-1 text-xs text-red-600">✗ {favoriteValidationResult.message}</p>
										{/if}
									{/if}
								{/if}

								<!-- 下载所有季度（仅番剧时显示，紧跟在Season ID后面） -->
								{#if sourceType === 'bangumi' && sourceId && bangumiSeasons.length > 0 && !loadingSeasons}
									<div class="mt-3 flex items-center space-x-2">
										<input
											type="checkbox"
											id="download-all-seasons"
											bind:checked={downloadAllSeasons}
											onchange={() => {
												if (downloadAllSeasons) selectedSeasons = [];
											}}
											class="text-primary focus:ring-primary h-4 w-4 rounded border-gray-300"
										/>
										<Label
											for="download-all-seasons"
											class="text-sm leading-none font-medium peer-disabled:cursor-not-allowed peer-disabled:opacity-70"
										>
											下载所有季度
										</Label>
									</div>
									{#if downloadAllSeasons}
										<p class="mt-1 ml-6 text-xs text-purple-600">
											勾选后将下载该番剧的所有季度，无需单独选择
										</p>
									{:else if bangumiSeasons.length > 1}
										<p class="mt-1 ml-6 text-xs text-purple-600">
											检测到 {bangumiSeasons.length} 个相关季度，请在{isMobile
												? '下方'
												: '右侧'}选择要下载的季度
										</p>
									{:else if bangumiSeasons.length === 1}
										<p class="mt-1 ml-6 text-xs text-purple-600">该番剧只有当前一个季度</p>
									{/if}

									<!-- 合并到现有番剧源选项 -->
									{#if existingBangumiSources.length > 0}
										<div class="mt-3 space-y-2">
											<Label class="text-sm font-medium">合并选项（可选）</Label>
											<select
												bind:value={mergeToSourceId}
												class="w-full rounded-md border border-gray-300 bg-white px-3 py-2 text-sm focus:border-purple-500 focus:ring-1 focus:ring-purple-500 dark:border-gray-600 dark:bg-gray-800 dark:text-white"
											>
												<option value={null}>作为新的独立番剧源添加</option>
												{#each existingBangumiSources as source}
													<option value={source.id}>
														合并到：{source.name}
														{#if source.season_id}(Season ID: {source.season_id}){/if}
														{#if source.media_id}(Media ID: {source.media_id}){/if}
													</option>
												{/each}
											</select>
											{#if mergeToSourceId}
												<p class="text-xs text-orange-600">
													⚠️ 合并后，新番剧的内容将添加到选中的现有番剧源中，不会创建新的番剧源
												</p>
											{:else}
												<p class="text-xs text-gray-500">
													可以选择将新番剧合并到现有番剧源中，方便管理相关内容（如新季度、剧场版等）
												</p>
											{/if}
										</div>
									{/if}
								{:else if sourceType === 'bangumi' && sourceId && loadingSeasons}
									<p class="mt-3 text-xs text-purple-600">正在获取季度信息...</p>
								{/if}

								<!-- UP主投稿选择状态显示和控制（仅投稿类型时显示） -->
								{#if sourceType === 'submission' && sourceId}
									<div
										class="mt-3 rounded-lg border border-blue-200 bg-blue-50 p-3 dark:border-blue-800 dark:bg-blue-950"
									>
										<div class="flex items-center justify-between">
											<div>
												<span class="text-sm font-medium text-blue-800 dark:text-blue-200"
													>历史投稿选择</span
												>
												<span class="ml-2 text-xs text-blue-600 dark:text-blue-400">
													{#if selectedVideos.length > 0}
														已选择 {selectedVideos.length} 个历史投稿
													{:else}
														未选择特定投稿（将下载全部）
													{/if}
												</span>
											</div>
											<Button
												size="sm"
												variant="outline"
												onclick={() => {
													showSubmissionSelection = true;
												}}
												class="border-blue-300 text-blue-700 hover:bg-blue-100"
											>
												{selectedVideos.length > 0 ? '重新选择' : '选择投稿'}
											</Button>
										</div>
										<p class="mt-2 text-xs text-blue-600 dark:text-blue-400">
											💡
											您可以选择特定的历史投稿进行下载，未选择的视频将不会下载但会在数据库中记录。新发布的投稿会自动下载。
										</p>
									</div>
								{/if}
							</div>
						{/if}

						<!-- 名称 -->
						<div class="space-y-2">
							<Label for="name">名称</Label>
							<Input
								id="name"
								bind:value={name}
								placeholder="请输入视频源名称"
								required
								disabled={isMergingBangumi}
							/>
							{#if isMergingBangumi}
								<p class="text-xs text-purple-600">合并时自动沿用目标番剧源的名称</p>
							{/if}
						</div>

						<!-- 保存路径 -->
						<div class="space-y-2">
							<Label for="path">保存路径</Label>
							<Input
								id="path"
								bind:value={path}
								placeholder="例如：D:/Videos/Bilibili"
								required
								disabled={isMergingBangumi}
							/>
							{#if isMergingBangumi}
								<p class="text-xs text-purple-600">合并时自动沿用目标番剧源的保存路径</p>
							{:else}
								<p class="text-muted-foreground text-sm">请输入绝对路径</p>
							{/if}
						</div>

						<!-- 关键词过滤器（可折叠，双列表模式） -->
						<div class="space-y-2">
							<button
								type="button"
								onclick={() => (showKeywordSection = !showKeywordSection)}
								class="flex w-full items-center justify-between rounded-md border border-purple-200 bg-purple-50 px-3 py-2 text-left text-sm transition-colors hover:bg-purple-100 dark:border-purple-800 dark:bg-purple-950 dark:hover:bg-purple-900"
							>
								<div class="flex items-center gap-2">
									<FilterIcon class="h-4 w-4 text-purple-600 dark:text-purple-400" />
									<span class="font-medium text-purple-800 dark:text-purple-200">关键词过滤器</span>
									{#if blacklistKeywords.length > 0 || whitelistKeywords.length > 0}
										<span
											class="rounded-full bg-purple-600 px-2 py-0.5 text-xs text-white dark:bg-purple-500"
										>
											{blacklistKeywords.length + whitelistKeywords.length}
										</span>
									{/if}
								</div>
								<svg
									class="h-4 w-4 transform text-purple-600 transition-transform dark:text-purple-400 {showKeywordSection
										? 'rotate-180'
										: ''}"
									fill="none"
									stroke="currentColor"
									viewBox="0 0 24 24"
								>
									<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 9l-7 7-7-7" />
								</svg>
							</button>

							{#if showKeywordSection}
								<div
									class="space-y-3 rounded-md border border-purple-200 bg-purple-50/50 p-4 dark:border-purple-800 dark:bg-purple-950/50"
									transition:fly={{ y: -10, duration: 200 }}
								>
									<!-- 过滤逻辑说明 -->
									<div class="rounded-md border border-blue-200 bg-blue-50 p-2 dark:border-blue-800 dark:bg-blue-950">
										<p class="text-xs font-medium text-blue-800 dark:text-blue-200">过滤逻辑说明</p>
										<ul class="mt-1 space-y-0.5 text-xs text-blue-700 dark:text-blue-300">
											<li>1. 如果设置了白名单，视频必须匹配至少一个白名单关键词才会被下载</li>
											<li>2. 匹配黑名单的视频会被排除（即使通过了白名单）</li>
											<li>3. 同一关键词不能同时出现在黑名单和白名单中</li>
										</ul>
									</div>

									<!-- 双列表布局 -->
									<div class="grid grid-cols-1 md:grid-cols-2 gap-3">
										<!-- 白名单区域 -->
										<div class="space-y-2 rounded-md border border-green-200 bg-green-50 p-3 dark:border-green-800 dark:bg-green-950">
											<div class="flex items-center gap-2">
												<svg class="h-4 w-4 text-green-600 dark:text-green-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
													<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" />
												</svg>
												<span class="text-xs font-medium text-green-800 dark:text-green-200">白名单</span>
												<span class="text-xs text-green-600 dark:text-green-400">({whitelistKeywords.length})</span>
											</div>
											<p class="text-[10px] text-green-700 dark:text-green-300">只下载匹配的视频（留空则不限制）</p>

											<!-- 添加白名单关键词 -->
											<div class="flex gap-1">
												<Input
													bind:value={newWhitelistKeyword}
													placeholder="输入关键词"
													onkeydown={handleWhitelistKeywordKeydown}
													disabled={validatingWhitelistKeyword}
													class="flex-1 h-8 text-xs"
												/>
												<Button
													type="button"
													size="sm"
													onclick={addWhitelistKeyword}
													disabled={!newWhitelistKeyword.trim() || validatingWhitelistKeyword}
													class="bg-green-600 hover:bg-green-700 h-8 px-2 text-xs"
												>
													{validatingWhitelistKeyword ? '...' : '添加'}
												</Button>
											</div>
											{#if whitelistValidationError}
												<p class="text-[10px] text-red-500">{whitelistValidationError}</p>
											{/if}

											<!-- 白名单列表 -->
											<div class="max-h-24 space-y-1 overflow-y-auto">
												{#if whitelistKeywords.length === 0}
													<p class="text-[10px] text-green-600 dark:text-green-400 italic">暂无白名单关键词</p>
												{:else}
													{#each whitelistKeywords as keyword, index}
														<div class="flex items-center justify-between rounded bg-green-100 px-2 py-1 dark:bg-green-900">
															<code class="flex-1 truncate text-[10px] text-green-800 dark:text-green-200">{keyword}</code>
															<button
																type="button"
																onclick={() => removeWhitelistKeyword(index)}
																class="ml-1 flex-shrink-0 rounded p-0.5 text-green-600 hover:bg-green-200 hover:text-red-600 dark:hover:bg-green-800"
																title="删除"
															>
																<X class="h-3 w-3" />
															</button>
														</div>
													{/each}
												{/if}
											</div>
										</div>

										<!-- 黑名单区域 -->
										<div class="space-y-2 rounded-md border border-red-200 bg-red-50 p-3 dark:border-red-800 dark:bg-red-950">
											<div class="flex items-center gap-2">
												<svg class="h-4 w-4 text-red-600 dark:text-red-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
													<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M18.364 18.364A9 9 0 005.636 5.636m12.728 12.728A9 9 0 015.636 5.636m12.728 12.728L5.636 5.636" />
												</svg>
												<span class="text-xs font-medium text-red-800 dark:text-red-200">黑名单</span>
												<span class="text-xs text-red-600 dark:text-red-400">({blacklistKeywords.length})</span>
											</div>
											<p class="text-[10px] text-red-700 dark:text-red-300">排除匹配的视频（优先级高于白名单）</p>

											<!-- 添加黑名单关键词 -->
											<div class="flex gap-1">
												<Input
													bind:value={newBlacklistKeyword}
													placeholder="输入关键词"
													onkeydown={handleBlacklistKeywordKeydown}
													disabled={validatingBlacklistKeyword}
													class="flex-1 h-8 text-xs"
												/>
												<Button
													type="button"
													size="sm"
													onclick={addBlacklistKeyword}
													disabled={!newBlacklistKeyword.trim() || validatingBlacklistKeyword}
													class="bg-red-600 hover:bg-red-700 h-8 px-2 text-xs"
												>
													{validatingBlacklistKeyword ? '...' : '添加'}
												</Button>
											</div>
											{#if blacklistValidationError}
												<p class="text-[10px] text-red-500">{blacklistValidationError}</p>
											{/if}

											<!-- 黑名单列表 -->
											<div class="max-h-24 space-y-1 overflow-y-auto">
												{#if blacklistKeywords.length === 0}
													<p class="text-[10px] text-red-600 dark:text-red-400 italic">暂无黑名单关键词</p>
												{:else}
													{#each blacklistKeywords as keyword, index}
														<div class="flex items-center justify-between rounded bg-red-100 px-2 py-1 dark:bg-red-900">
															<code class="flex-1 truncate text-[10px] text-red-800 dark:text-red-200">{keyword}</code>
															<button
																type="button"
																onclick={() => removeBlacklistKeyword(index)}
																class="ml-1 flex-shrink-0 rounded p-0.5 text-red-600 hover:bg-red-200 hover:text-red-800 dark:hover:bg-red-800"
																title="删除"
															>
																<X class="h-3 w-3" />
															</button>
														</div>
													{/each}
												{/if}
											</div>
										</div>
									</div>

									<!-- 正则表达式示例 -->
									<div class="rounded border border-purple-200 bg-white p-2 dark:border-purple-700 dark:bg-gray-800">
										<p class="text-xs font-medium text-purple-700 dark:text-purple-300">正则表达式示例：</p>
										<ul class="mt-1 space-y-0.5 text-[10px] text-purple-600 dark:text-purple-400">
											<li>
												<code class="rounded bg-purple-100 px-1 dark:bg-purple-800">PV</code> - 匹配包含"PV"的标题
											</li>
											<li>
												<code class="rounded bg-purple-100 px-1 dark:bg-purple-800">预告</code> - 匹配包含"预告"的标题
											</li>
											<li>
												<code class="rounded bg-purple-100 px-1 dark:bg-purple-800">第\d+期</code> - 匹配"第N期"格式
											</li>
										</ul>
										<p class="mt-1 text-[10px] text-purple-500 dark:text-purple-400">
											示例：白名单添加"PV"，黑名单添加"预告"，则下载含"PV"但不含"预告"的视频
										</p>
									</div>
								</div>
							{/if}
						</div>

						<!-- 提交按钮 -->
						<div class="flex {isMobile ? 'flex-col' : ''} gap-2">
							<Button type="submit" disabled={loading} class={isMobile ? 'w-full' : ''}>
								{loading ? '添加中...' : '添加'}
							</Button>
							<Button
								type="button"
								variant="outline"
								onclick={() => goto('/')}
								class={isMobile ? 'w-full' : ''}
							>
								取消
							</Button>
						</div>
					</form>
				</div>

				<!-- 右侧：搜索结果区域 -->
				{#if showSearchResults && searchResults.length > 0}
					<div
						class={isMobile ? 'mt-6 w-full' : 'min-w-[550px] flex-1'}
						transition:fly={{ x: 300, duration: 300 }}
					>
						<div
							class="bg-card rounded-lg border {isMobile
								? ''
								: 'h-full'} flex flex-col overflow-hidden {isMobile
								? ''
								: 'sticky top-6'} max-h-[calc(100vh-200px)]"
						>
							<div class="bg-muted flex items-center justify-between border-b p-4">
								<div>
									<span class="text-foreground text-base font-medium">搜索结果</span>
									<span class="text-muted-foreground text-sm {isMobile ? 'block' : 'ml-2'}">
										共找到 {searchTotalResults} 个结果
									</span>
								</div>
								<div class="flex items-center gap-2">
									{#if batchMode && sourceType === 'submission'}
										<Button
											size="sm"
											variant="outline"
											onclick={() => selectAllVisible('search')}
											class="text-xs"
										>
											全选
										</Button>
									{/if}
									<button
										onclick={() => {
											showSearchResults = false;
											searchResults = [];
											searchTotalResults = 0;
										}}
										class="text-muted-foreground hover:text-foreground p-1 text-xl"
									>
										<X class="h-5 w-5" />
									</button>
								</div>
							</div>

							<div class="flex-1 overflow-hidden p-3">
								<div class="seasons-grid-container h-full">
									<div
										class="grid gap-4 {isMobile ? 'grid-cols-1' : ''}"
										style={isMobile
											? ''
											: 'grid-template-columns: repeat(auto-fit, minmax(250px, 1fr));'}
									>
										{#each filteredSearchResults as result, i (result.bvid || result.season_id || result.mid || i)}
											{@const isBangumiExisting =
												sourceType === 'bangumi' &&
												result.season_id &&
												isBangumiSeasonExists(result.season_id)}
											{@const itemKey = `search_${result.bvid || result.season_id || result.mid || i}`}
											<button
												onclick={() => {
													if (batchMode && sourceType === 'submission') {
														toggleBatchSelection(itemKey, result, 'search');
													} else {
														selectSearchResult(result);
													}
												}}
												onmouseenter={(e) => handleMouseEnter(result, e)}
												onmouseleave={handleMouseLeave}
												onmousemove={handleMouseMove}
												class="hover:bg-muted relative flex transform items-start gap-3 rounded-lg border p-4 text-left transition-all duration-300 hover:scale-102 hover:shadow-md {isBangumiExisting
													? 'opacity-60'
													: ''} {batchMode && isBatchSelected(itemKey)
													? 'bg-blue-50 ring-2 ring-blue-500 dark:bg-blue-950'
													: ''}"
												transition:fly={{ y: 50, duration: 300, delay: i * 50 }}
												animate:flip={{ duration: 300 }}
												disabled={isBangumiExisting}
											>
												<!-- 批量模式下的复选框 -->
												{#if batchMode && sourceType === 'submission'}
													<div class="flex-shrink-0 pt-1">
														<input
															type="checkbox"
															checked={batchCheckboxStates[itemKey] || false}
															onclick={(e) => {
																e.stopPropagation();
																toggleBatchSelection(itemKey, result, 'search');
															}}
															class="h-4 w-4 rounded border-gray-300 text-blue-600 focus:ring-blue-500"
														/>
													</div>
												{/if}
												{#if result.cover}
													<img
														src={processBilibiliImageUrl(result.cover)}
														alt={result.title}
														class="{sourceType === 'bangumi'
															? 'h-20 w-14'
															: 'h-14 w-20'} flex-shrink-0 rounded object-cover"
														onerror={handleImageError}
														loading="lazy"
														crossorigin="anonymous"
														referrerpolicy="no-referrer"
													/>
												{:else}
													<div
														class="{sourceType === 'bangumi'
															? 'h-20 w-14'
															: 'h-14 w-20'} bg-muted text-muted-foreground flex flex-shrink-0 items-center justify-center rounded text-xs"
													>
														无图片
													</div>
												{/if}
												<div class="min-w-0 flex-1">
													<div class="mb-1 flex items-center gap-2">
														<h4 class="text-foreground flex-1 truncate text-sm font-medium">
															<!-- eslint-disable-next-line svelte/no-at-html-tags -->
															{@html result.title}
														</h4>
														{#if result.result_type}
															<span
																class="flex-shrink-0 rounded px-1.5 py-0.5 text-xs {result.result_type ===
																'media_bangumi'
																	? 'bg-purple-100 text-purple-700 dark:bg-purple-900 dark:text-purple-300'
																	: result.result_type === 'media_ft'
																		? 'bg-red-100 text-red-700 dark:bg-red-900 dark:text-red-300'
																		: result.result_type === 'bili_user'
																			? 'bg-blue-100 text-blue-700 dark:bg-blue-900 dark:text-blue-300'
																			: result.result_type === 'video'
																				? 'bg-green-100 text-green-700 dark:bg-green-900 dark:text-green-300'
																				: 'text-foreground bg-gray-100 dark:bg-gray-800'}"
															>
																{result.result_type === 'media_bangumi'
																	? '番剧'
																	: result.result_type === 'media_ft'
																		? '影视'
																		: result.result_type === 'bili_user'
																			? 'UP主'
																			: result.result_type === 'video'
																				? '视频'
																				: result.result_type}
															</span>
														{/if}
														<!-- 显示已存在标记 -->
														{#if sourceType === 'submission' && result.mid && isSubmissionExists(Number(result.mid))}
															<span
																class="flex-shrink-0 rounded bg-gray-100 px-1.5 py-0.5 text-xs text-gray-700 dark:bg-gray-800 dark:text-gray-300"
															>
																已添加
															</span>
														{/if}
														{#if isBangumiExisting}
															<span
																class="flex-shrink-0 rounded bg-gray-100 px-1.5 py-0.5 text-xs text-gray-700 dark:bg-gray-800 dark:text-gray-300"
															>
																已添加
															</span>
														{/if}
													</div>
													<p class="text-muted-foreground truncate text-xs">{result.author}</p>
													{#if result.description}
														<p class="text-muted-foreground/70 mt-1 line-clamp-2 text-xs">
															{result.description}
														</p>
													{/if}
												</div>
											</button>
										{/each}
									</div>
								</div>
							</div>

							{#if searchResults.length > 0}
								<div class="border-t p-3 text-center">
									<span class="text-muted-foreground text-xs">
										共显示 {searchResults.length} 个结果
										{#if searchTotalResults > searchResults.length}
											（总共 {searchTotalResults} 个）
										{/if}
									</span>
								</div>
							{/if}
						</div>
					</div>
				{/if}

				<!-- 关注UP主列表（移动到右侧） -->
				{#if (sourceType === 'collection' || sourceType === 'submission') && userFollowings.length > 0}
					<div class={isMobile ? 'mt-6 w-full' : 'flex-1'}>
						<div
							class="bg-card rounded-lg border {isMobile
								? ''
								: 'h-full'} flex flex-col overflow-hidden {isMobile
								? ''
								: 'sticky top-6'} max-h-126"
						>
							<div
								class="flex items-center justify-between border-b bg-blue-50 p-4 dark:bg-blue-950"
							>
								<div>
									<span class="text-base font-medium text-blue-800 dark:text-blue-200"
										>关注的UP主</span
									>
									<span
										class="text-sm text-blue-600 dark:text-blue-400 {isMobile ? 'block' : 'ml-2'}"
									>
										共 {userFollowings.length} 个UP主
									</span>
								</div>
								{#if batchMode}
									<Button
										size="sm"
										variant="outline"
										onclick={() => selectAllVisible('following')}
										class="text-xs"
									>
										全选
									</Button>
								{/if}
							</div>

							<div class="flex-1 overflow-y-auto p-3">
								<div
									class="grid gap-3 {isMobile ? 'grid-cols-1' : ''}"
									style={isMobile
										? ''
										: 'grid-template-columns: repeat(auto-fit, minmax(250px, 1fr));'}
								>
									{#each filteredUserFollowings as following (following.mid)}
										{@const itemKey = `following_${following.mid}`}
										{@const isDisabled =
											sourceType === 'submission' && existingSubmissionIds.has(following.mid)}
										<button
											onclick={() => {
												if (batchMode) {
													toggleBatchSelection(itemKey, following, 'following');
												} else {
													selectFollowing(following);
												}
											}}
											disabled={isDisabled}
											class="hover:bg-muted rounded-lg border p-3 text-left transition-colors {isDisabled
												? 'cursor-not-allowed opacity-60'
												: ''} {batchMode && isBatchSelected(itemKey)
												? 'bg-blue-50 ring-2 ring-blue-500 dark:bg-blue-950'
												: ''}"
										>
											<div class="flex items-start gap-2">
												<!-- 批量模式下的复选框 -->
												{#if batchMode}
													<div class="flex-shrink-0 pt-1">
														<input
															type="checkbox"
															checked={batchCheckboxStates[itemKey] || false}
															onclick={(e) => {
																e.stopPropagation();
																toggleBatchSelection(itemKey, following, 'following');
															}}
															class="h-4 w-4 rounded border-gray-300 text-blue-600 focus:ring-blue-500"
														/>
													</div>
												{/if}
												{#if following.face}
													<img
														src={processBilibiliImageUrl(following.face)}
														alt={following.name}
														class="h-10 w-10 flex-shrink-0 rounded-full object-cover"
														onerror={handleImageError}
														loading="lazy"
														crossorigin="anonymous"
														referrerpolicy="no-referrer"
													/>
												{:else}
													<div
														class="bg-muted text-muted-foreground flex h-10 w-10 flex-shrink-0 items-center justify-center rounded-full text-xs"
													>
														头像
													</div>
												{/if}
												<div class="min-w-0 flex-1">
													<div class="mb-1 flex items-center gap-1">
														<h4 class="truncate text-xs font-medium">{following.name}</h4>
														{#if following.official_verify && following.official_verify.type >= 0}
															<span
																class="flex-shrink-0 rounded bg-yellow-100 px-1 py-0.5 text-xs text-yellow-700 dark:bg-yellow-900 dark:text-yellow-300"
															>
																V
															</span>
														{/if}
														{#if sourceType === 'submission' && existingSubmissionIds.has(following.mid)}
															<span
																class="flex-shrink-0 rounded bg-gray-100 px-1 py-0.5 text-xs text-gray-700 dark:bg-gray-800 dark:text-gray-300"
															>
																已添加
															</span>
														{/if}
													</div>
													<p class="text-muted-foreground mb-1 truncate text-xs">
														UID: {following.mid}
													</p>
													{#if following.sign}
														<p class="text-muted-foreground line-clamp-1 text-xs">
															{following.sign}
														</p>
													{/if}
												</div>
											</div>
										</button>
									{/each}
								</div>
							</div>
						</div>
					</div>
				{/if}

				<!-- UP主合集列表（移动到右侧） -->
				{#if sourceType === 'collection' && userCollections.length > 0}
					<div class={isMobile ? 'mt-6 w-full' : 'flex-1'}>
						<div
							class="bg-card rounded-lg border {isMobile
								? ''
								: 'h-full'} flex flex-col overflow-hidden {isMobile
								? ''
								: 'sticky top-6'} max-h-[calc(100vh-200px)]"
						>
							<div
								class="flex items-center justify-between border-b bg-green-50 p-4 dark:bg-green-950"
							>
								<div>
									<span class="text-base font-medium text-green-800 dark:text-green-200"
										>UP主合集列表</span
									>
									<span
										class="text-sm text-green-600 dark:text-green-400 {isMobile ? 'block' : 'ml-2'}"
									>
										共 {userCollections.length} 个合集
									</span>
								</div>
								{#if batchMode}
									<Button
										size="sm"
										variant="outline"
										onclick={() => selectAllVisible('collection')}
										class="text-xs"
									>
										全选
									</Button>
								{/if}
							</div>

							<div class="flex-1 overflow-y-auto p-3">
								<div
									class="grid gap-4 {isMobile ? 'grid-cols-1' : ''}"
									style={isMobile
										? ''
										: 'grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));'}
								>
									{#each filteredUserCollections as collection (collection.sid)}
										{@const itemKey = `collection_${collection.sid}`}
										{@const isDisabled = isCollectionExists(
											collection.sid,
											collection.mid.toString()
										)}
										<button
											onclick={() => {
												if (batchMode) {
													toggleBatchSelection(itemKey, collection, 'collection');
												} else {
													selectCollection(collection);
												}
											}}
											disabled={isDisabled}
											class="hover:bg-muted rounded-lg border p-4 text-left transition-colors {isDisabled
												? 'cursor-not-allowed opacity-60'
												: ''} {batchMode && isBatchSelected(itemKey)
												? 'bg-blue-50 ring-2 ring-blue-500 dark:bg-blue-950'
												: ''}"
										>
											<div class="flex items-start gap-3">
												<!-- 批量模式下的复选框 -->
												{#if batchMode}
													<div class="flex-shrink-0 pt-1">
														<input
															type="checkbox"
															checked={batchCheckboxStates[itemKey] || false}
															onclick={(e) => {
																e.stopPropagation();
																toggleBatchSelection(itemKey, collection, 'collection');
															}}
															class="h-4 w-4 rounded border-gray-300 text-blue-600 focus:ring-blue-500"
														/>
													</div>
												{/if}
												{#if collection.cover}
													<img
														src={processBilibiliImageUrl(collection.cover)}
														alt={collection.name}
														class="h-16 w-24 flex-shrink-0 rounded object-cover"
														onerror={handleImageError}
														loading="lazy"
														crossorigin="anonymous"
														referrerpolicy="no-referrer"
													/>
												{:else}
													<div
														class="bg-muted text-muted-foreground flex h-16 w-24 flex-shrink-0 items-center justify-center rounded text-xs"
													>
														无封面
													</div>
												{/if}
												<div class="min-w-0 flex-1">
													<div class="mb-1 flex items-center gap-2">
														<h4 class="truncate text-sm font-medium">{collection.name}</h4>
														<span
															class="flex-shrink-0 rounded px-2 py-0.5 text-xs {collection.collection_type ===
															'season'
																? 'bg-green-100 text-green-700 dark:bg-green-900 dark:text-green-300'
																: 'bg-blue-100 text-blue-700'}"
														>
															{collection.collection_type === 'season' ? '合集' : '系列'}
														</span>
														{#if isCollectionExists(collection.sid, collection.mid.toString())}
															<span
																class="flex-shrink-0 rounded bg-gray-100 px-2 py-0.5 text-xs text-gray-700 dark:bg-gray-800 dark:text-gray-300"
															>
																已添加
															</span>
														{/if}
													</div>
													<p class="text-muted-foreground mb-1 text-xs">
														ID: {collection.sid} (检查key: {collection.sid}_{collection.mid})
													</p>
													<p class="text-muted-foreground text-xs">共 {collection.total} 个视频</p>
													{#if collection.description}
														<p class="text-muted-foreground mt-1 line-clamp-2 text-xs">
															{collection.description}
														</p>
													{/if}
												</div>
											</div>
										</button>
									{/each}
								</div>
							</div>
						</div>
					</div>
				{/if}

				<!-- 收藏夹列表（移动到右侧） -->
				{#if sourceType === 'favorite' && userFavorites.length > 0}
					<div class={isMobile ? 'mt-6 w-full' : 'flex-1'}>
						<div
							class="bg-card rounded-lg border {isMobile
								? ''
								: 'h-full'} flex flex-col overflow-hidden {isMobile
								? ''
								: 'sticky top-6'} max-h-[calc(100vh-200px)]"
						>
							<div
								class="flex items-center justify-between border-b bg-yellow-50 p-4 dark:bg-yellow-950"
							>
								<div>
									<span class="text-base font-medium text-yellow-800 dark:text-yellow-200"
										>我的收藏夹</span
									>
									<span
										class="text-sm text-yellow-600 dark:text-yellow-400 {isMobile
											? 'block'
											: 'ml-2'}"
									>
										共 {userFavorites.length} 个收藏夹
									</span>
								</div>
								{#if batchMode}
									<Button
										size="sm"
										variant="outline"
										onclick={() => selectAllVisible('favorite')}
										class="text-xs"
									>
										全选
									</Button>
								{/if}
							</div>

							<div class="flex-1 overflow-y-auto p-3">
								<div
									class="grid gap-4 {isMobile ? 'grid-cols-1' : ''}"
									style={isMobile
										? ''
										: 'grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));'}
								>
									{#each filteredUserFavorites as favorite (favorite.id)}
										{@const itemKey = `favorite_${favorite.id}`}
										{@const isDisabled = existingFavoriteIds.has(Number(favorite.id))}
										<button
											onclick={() => {
												if (batchMode) {
													toggleBatchSelection(itemKey, favorite, 'favorite');
												} else {
													selectFavorite(favorite);
												}
											}}
											disabled={isDisabled}
											class="hover:bg-muted rounded-lg border p-4 text-left transition-colors {isDisabled
												? 'cursor-not-allowed opacity-60'
												: ''} {batchMode && isBatchSelected(itemKey)
												? 'bg-blue-50 ring-2 ring-blue-500 dark:bg-blue-950'
												: ''}"
										>
											<div class="flex items-start gap-3">
												<!-- 批量模式下的复选框 -->
												{#if batchMode}
													<div class="flex-shrink-0 pt-1">
														<input
															type="checkbox"
															checked={batchCheckboxStates[itemKey] || false}
															onclick={(e) => {
																e.stopPropagation();
																toggleBatchSelection(itemKey, favorite, 'favorite');
															}}
															class="h-4 w-4 rounded border-gray-300 text-blue-600 focus:ring-blue-500"
														/>
													</div>
												{/if}
												{#if favorite.cover}
													<img
														src={processBilibiliImageUrl(favorite.cover)}
														alt={favorite.name || favorite.title}
														class="h-16 w-24 flex-shrink-0 rounded object-cover"
														onerror={handleImageError}
														loading="lazy"
														crossorigin="anonymous"
														referrerpolicy="no-referrer"
													/>
												{:else}
													<div
														class="bg-muted text-muted-foreground flex h-16 w-24 flex-shrink-0 items-center justify-center rounded text-xs"
													>
														无封面
													</div>
												{/if}
												<div class="min-w-0 flex-1">
													<div class="mb-1 flex items-center gap-2">
														<h4 class="truncate text-sm font-medium">
															{favorite.name || favorite.title}
														</h4>
														{#if existingFavoriteIds.has(Number(favorite.id))}
															<span
																class="flex-shrink-0 rounded bg-gray-100 px-2 py-0.5 text-xs text-gray-700 dark:bg-gray-800 dark:text-gray-300"
															>
																已添加
															</span>
														{/if}
													</div>
													<p class="text-muted-foreground mb-1 text-xs">收藏夹ID: {favorite.id}</p>
													<p class="text-muted-foreground mb-1 text-xs">
														共 {favorite.media_count} 个视频
													</p>
													{#if favorite.created}
														<p class="text-muted-foreground text-xs">
															创建于 {new Date(favorite.created * 1000).toLocaleDateString()}
														</p>
													{/if}
												</div>
											</div>
										</button>
									{/each}
								</div>
							</div>
						</div>
					</div>
				{/if}

				<!-- UP主收藏夹列表（移动到右侧） -->
				{#if sourceType === 'favorite' && selectedUserId && (searchedUserFavorites.length > 0 || loadingSearchedUserFavorites)}
					<div class={isMobile ? 'mt-6 w-full' : 'flex-1'}>
						<div
							class="bg-card rounded-lg border {isMobile
								? ''
								: 'h-full'} flex flex-col overflow-hidden {isMobile
								? ''
								: 'sticky top-6'} max-h-[calc(100vh-200px)]"
						>
							<div
								class="flex items-center justify-between border-b bg-green-50 p-4 dark:bg-green-950"
							>
								<div>
									<span class="text-base font-medium text-green-800 dark:text-green-200"
										>{selectedUserName} 的收藏夹</span
									>
									<span
										class="text-sm text-green-600 dark:text-green-400 {isMobile ? 'block' : 'ml-2'}"
									>
										{#if loadingSearchedUserFavorites}
											正在加载...
										{:else if searchedUserFavorites.length > 0}
											共 {searchedUserFavorites.length} 个收藏夹
										{:else}
											没有公开收藏夹
										{/if}
									</span>
								</div>
								<div class="flex items-center gap-2">
									{#if batchMode && searchedUserFavorites.length > 0}
										<Button
											size="sm"
											variant="outline"
											onclick={() => selectAllVisible('searched-favorite')}
											class="text-xs"
										>
											全选
										</Button>
									{/if}
									<button
										onclick={() => {
											selectedUserId = '';
											selectedUserName = '';
											searchedUserFavorites = [];
											loadingSearchedUserFavorites = false;
										}}
										class="p-1 text-xl text-green-500 hover:text-green-700 dark:text-green-300"
									>
										<X class="h-5 w-5" />
									</button>
								</div>
							</div>

							<div class="flex-1 overflow-y-auto p-3">
								{#if loadingSearchedUserFavorites}
									<div class="p-4 text-center">
										<div class="text-sm text-green-700 dark:text-green-300">
											正在获取收藏夹列表...
										</div>
									</div>
								{:else if searchedUserFavorites.length > 0}
									<div
										class="grid gap-4 {isMobile ? 'grid-cols-1' : ''}"
										style={isMobile
											? ''
											: 'grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));'}
									>
										{#each filteredSearchedUserFavorites as favorite (favorite.id)}
											{@const itemKey = `searched-favorite_${favorite.id}`}
												{@const isDisabled = existingFavoriteIds.has(Number(favorite.id))}
											<button
												onclick={() => {
													if (batchMode) {
														toggleBatchSelection(itemKey, favorite, 'favorite');
													} else {
														selectSearchedFavorite(favorite);
													}
												}}
												disabled={isDisabled}
												class="hover:bg-muted rounded-lg border p-4 text-left transition-colors {isDisabled
													? 'cursor-not-allowed opacity-60'
													: ''} {batchMode && isBatchSelected(itemKey)
													? 'bg-blue-50 ring-2 ring-blue-500 dark:bg-blue-950'
													: ''}"
											>
												<div class="flex items-start gap-3">
													<!-- 批量模式下的复选框 -->
													{#if batchMode}
														<div class="flex-shrink-0 pt-1">
															<input
																type="checkbox"
																checked={batchCheckboxStates[itemKey] || false}
																onclick={(e) => {
																	e.stopPropagation();
																	toggleBatchSelection(itemKey, favorite, 'favorite');
																}}
																class="h-4 w-4 rounded border-gray-300 text-blue-600 focus:ring-blue-500"
															/>
														</div>
													{/if}
													<div
														class="bg-muted text-muted-foreground flex h-16 w-24 flex-shrink-0 items-center justify-center rounded text-xs"
													>
														收藏夹
													</div>
													<div class="min-w-0 flex-1">
														<div class="mb-1 flex items-center gap-2">
															<h4 class="truncate text-sm font-medium">{favorite.title}</h4>
															{#if existingFavoriteIds.has(Number(favorite.id))}
																<span
																	class="flex-shrink-0 rounded bg-gray-100 px-2 py-0.5 text-xs text-gray-700 dark:bg-gray-800 dark:text-gray-300"
																>
																	已添加
																</span>
															{/if}
														</div>
														<p class="text-muted-foreground mb-1 text-xs">
															收藏夹ID: {favorite.id}
														</p>
														<p class="text-muted-foreground text-xs">
															共 {favorite.media_count} 个视频
														</p>
													</div>
												</div>
											</button>
										{/each}
									</div>
								{:else}
									<div class="p-4 text-center">
										<div class="text-muted-foreground text-sm">该UP主没有公开的收藏夹</div>
									</div>
								{/if}
							</div>
						</div>
					</div>
				{/if}

				<!-- 番剧季度选择区域（移动到右侧） -->
				{#if sourceType === 'bangumi' && sourceId && !downloadAllSeasons && bangumiSeasons.length > 1}
					<div class={isMobile ? 'mt-6 w-full' : 'flex-1'}>
						<div
							class="bg-card rounded-lg border {isMobile
								? ''
								: 'h-full'} flex flex-col overflow-hidden {isMobile
								? ''
								: 'sticky top-6'} max-h-[calc(100vh-200px)]"
						>
							<div
								class="flex items-center justify-between border-b bg-purple-50 p-4 dark:bg-purple-950"
							>
								<div>
									<span class="text-base font-medium text-purple-800 dark:text-purple-200"
										>选择要下载的季度</span
									>
									<span
										class="text-sm text-purple-600 dark:text-purple-400 {isMobile
											? 'block'
											: 'ml-2'}"
									>
										{#if loadingSeasons}
											正在加载...
										{:else if bangumiSeasons.length > 0}
											共 {bangumiSeasons.length} 个相关季度
										{:else}
											暂无季度信息
										{/if}
									</span>
								</div>
								{#if selectedSeasons.length > 0}
									<span
										class="rounded bg-purple-100 px-2 py-1 text-xs text-purple-700 dark:bg-purple-900 dark:text-purple-300"
									>
										已选择 {selectedSeasons.length} 个
										{#if selectedSeasons.length === bangumiSeasons.length}
											（全部）
										{/if}
									</span>
								{/if}
							</div>

							<div class="flex-1 overflow-hidden p-3">
								{#if loadingSeasons}
									<div class="p-4 text-center">
										<div class="text-sm text-purple-700 dark:text-purple-300">
											正在加载季度信息...
										</div>
									</div>
								{:else if bangumiSeasons.length > 0}
									<div class="seasons-grid-container">
										<div
											class="grid gap-4 {isMobile ? 'grid-cols-1' : ''}"
											style={isMobile
												? ''
												: 'grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));'}
										>
											{#each filteredBangumiSeasons as season, i (season.season_id)}
												<div
													role="button"
													tabindex="0"
													class="relative rounded-lg border p-4 transition-all duration-300 {season.isExisting
														? 'cursor-not-allowed bg-gray-50 opacity-60 dark:bg-gray-800'
														: 'transform cursor-pointer hover:scale-102 hover:bg-purple-50 hover:shadow-md dark:hover:bg-purple-900'} {isMobile
														? 'h-auto'
														: 'h-[120px]'}"
													onmouseenter={(e) =>
														!season.isExisting && handleSeasonMouseEnter(season, e)}
													onmouseleave={!season.isExisting ? handleSeasonMouseLeave : undefined}
													onmousemove={!season.isExisting ? handleSeasonMouseMove : undefined}
													onclick={() =>
														!season.isExisting && toggleSeasonSelection(season.season_id)}
													onkeydown={(e) =>
														!season.isExisting &&
														(e.key === 'Enter' || e.key === ' ') &&
														toggleSeasonSelection(season.season_id)}
													transition:fly={{ y: 50, duration: 300, delay: i * 100 }}
													animate:flip={{ duration: 300 }}
												>
													<div class="flex gap-3 {isMobile ? '' : 'h-full'}">
														{#if season.cover}
															<img
																src={processBilibiliImageUrl(season.cover)}
																alt={season.season_title || season.title}
																class="h-20 w-14 flex-shrink-0 rounded object-cover"
																onerror={handleImageError}
																loading="lazy"
																crossorigin="anonymous"
																referrerpolicy="no-referrer"
															/>
														{:else}
															<div
																class="bg-muted text-muted-foreground flex h-20 w-14 flex-shrink-0 items-center justify-center rounded text-xs"
															>
																无封面
															</div>
														{/if}
														<div class="min-w-0 flex-1">
															<div class="absolute top-3 right-3">
																<input
																	type="checkbox"
																	id="season-{season.season_id}"
																	checked={selectedSeasons.includes(season.season_id)}
																	disabled={season.isExisting}
																	onchange={() => toggleSeasonSelection(season.season_id)}
																	class="h-4 w-4 rounded border-gray-300 text-purple-600 focus:ring-purple-500 {season.isExisting
																		? 'cursor-not-allowed opacity-50'
																		: ''}"
																/>
															</div>
															<!-- 右下角集数标签 -->
															{#if season.episode_count}
																<div class="absolute right-3 bottom-3">
																	<span
																		class="rounded bg-purple-100 px-1.5 py-0.5 text-xs text-purple-700 dark:bg-purple-900 dark:text-purple-300"
																		>{season.episode_count}集</span
																	>
																</div>
															{/if}
															<label for="season-{season.season_id}" class="cursor-pointer">
																<h4 class="truncate pr-6 text-sm font-medium">
																	{season.full_title || season.season_title || season.title}
																</h4>
																{#if season.season_id === sourceId}
																	<span
																		class="mt-1 inline-block rounded bg-purple-100 px-1.5 py-0.5 text-xs text-purple-700 dark:bg-purple-900 dark:text-purple-300"
																		>当前</span
																	>
																{/if}
																{#if season.isExisting}
																	<span
																		class="mt-1 ml-1 inline-block rounded bg-gray-100 px-1.5 py-0.5 text-xs text-gray-700 dark:bg-gray-800 dark:text-gray-300"
																		>已添加</span
																	>
																{/if}
																<p class="text-muted-foreground mt-1 text-xs">
																	Season ID: {season.season_id}
																</p>
																{#if season.media_id}
																	<p class="text-muted-foreground text-xs">
																		Media ID: {season.media_id}
																	</p>
																{/if}
															</label>
														</div>
													</div>
												</div>
											{/each}
										</div>
									</div>
									{#if !loadingSeasons && bangumiSeasons.length > 0}
										<p class="mt-3 text-center text-xs text-purple-600">
											不选择则仅下载{isMobile ? '上方' : '左侧'}输入的当前季度
										</p>
									{/if}
								{:else if sourceId}
									<div class="p-4 text-center">
										<div class="text-muted-foreground text-sm">暂无季度信息</div>
										<div class="text-muted-foreground mt-1 text-xs">请检查Season ID是否正确</div>
									</div>
								{/if}
							</div>
						</div>
					</div>
				{/if}

				<!-- 订阅的合集列表（仅合集类型时显示） -->
				{#if sourceType === 'collection' && subscribedCollections.length > 0}
					<div class={isMobile ? 'mt-6 w-full' : 'flex-1'}>
						<div
							class="bg-card rounded-lg border {isMobile
								? ''
								: 'h-full'} flex flex-col overflow-hidden {isMobile ? '' : 'sticky top-6'} max-h-96"
						>
							<div
								class="flex items-center justify-between border-b bg-purple-50 p-4 dark:bg-purple-950"
							>
								<div>
									<span class="text-base font-medium text-purple-800 dark:text-purple-200"
										>关注的合集</span
									>
									<span
										class="text-sm text-purple-600 dark:text-purple-400 {isMobile
											? 'block'
											: 'ml-2'}"
									>
										共 {subscribedCollections.length} 个合集
									</span>
								</div>
								{#if batchMode}
									<Button
										size="sm"
										variant="outline"
										onclick={() => selectAllVisible('subscribed-collection')}
										class="text-xs"
									>
										全选
									</Button>
								{/if}
							</div>

							<div class="flex-1 overflow-y-auto p-3">
								<div
									class="grid gap-4 {isMobile ? 'grid-cols-1' : ''}"
									style={isMobile
										? ''
										: 'grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));'}
								>
									{#each subscribedCollections as collection (collection.sid)}
										{@const itemKey = `subscribed-collection_${collection.sid}`}
										{@const isExisting = collection.collection_type === 'favorite'
											? existingFavoriteIds.has(Number(collection.sid))
											: isCollectionExists(collection.sid, collection.up_mid.toString())}
										<button
											onclick={() => {
												if (batchMode) {
													toggleBatchSelection(itemKey, collection, collection.collection_type === 'favorite' ? 'favorite' : 'collection');
												} else {
													selectSubscribedCollection(collection);
												}
											}}
											disabled={isExisting}
											class="hover:bg-muted rounded-lg border p-4 text-left transition-colors {isExisting
												? 'cursor-not-allowed opacity-60'
												: ''} {batchMode && isBatchSelected(itemKey)
												? 'bg-blue-50 ring-2 ring-blue-500 dark:bg-blue-950'
												: ''}"
										>
											<div class="flex items-start gap-3">
												<!-- 批量模式下的复选框 -->
												{#if batchMode}
													<div class="flex-shrink-0 pt-1">
														<input
															type="checkbox"
															checked={batchCheckboxStates[itemKey] || false}
															onclick={(e) => {
																e.stopPropagation();
																toggleBatchSelection(itemKey, collection, collection.collection_type === 'favorite' ? 'favorite' : 'collection');
															}}
															class="h-4 w-4 rounded border-gray-300 text-blue-600 focus:ring-blue-500"
														/>
													</div>
												{/if}
												{#if collection.cover}
													<img
														src={processBilibiliImageUrl(collection.cover)}
														alt={collection.name}
														class="h-16 w-24 flex-shrink-0 rounded object-cover"
														onerror={handleImageError}
														loading="lazy"
														crossorigin="anonymous"
														referrerpolicy="no-referrer"
													/>
												{:else}
													<div
														class="bg-muted text-muted-foreground flex h-16 w-24 flex-shrink-0 items-center justify-center rounded text-xs"
													>
														{collection.collection_type === 'favorite' ? '收藏夹' : '无封面'}
													</div>
												{/if}
												<div class="min-w-0 flex-1">
													<div class="mb-1 flex items-center gap-2">
														<h4 class="truncate text-sm font-medium">{collection.name}</h4>
														<span
															class="flex-shrink-0 rounded px-2 py-0.5 text-xs {collection.collection_type === 'favorite'
																? 'bg-blue-100 text-blue-700 dark:bg-blue-900 dark:text-blue-300'
																: 'bg-purple-100 text-purple-700 dark:bg-purple-900 dark:text-purple-300'}"
														>
															{collection.collection_type === 'favorite' ? '收藏夹' : '合集'}
														</span>
														{#if isExisting}
															<span
																class="flex-shrink-0 rounded bg-gray-100 px-2 py-0.5 text-xs text-gray-700 dark:bg-gray-800 dark:text-gray-300"
															>
																已添加
															</span>
														{/if}
													</div>
													<p class="text-muted-foreground mb-1 text-xs">ID: {collection.sid}</p>
													<p class="text-muted-foreground mb-1 text-xs">
														UP主: {collection.up_name}
													</p>
													<p class="text-muted-foreground text-xs">共 {collection.total} 个视频</p>
													{#if collection.description}
														<p class="text-muted-foreground mt-1 line-clamp-2 text-xs">
															{collection.description}
														</p>
													{/if}
												</div>
											</div>
										</button>
									{/each}
								</div>
							</div>
						</div>
					</div>
				{/if}

				<!-- UP主投稿选择面板（仅投稿类型时显示） -->
				{#if sourceType === 'submission' && showSubmissionSelection}
					<div
						class={isMobile ? 'mt-6 w-full' : 'flex-1'}
						transition:fly={{ x: 300, duration: 300 }}
					>
						<div
							class="bg-card rounded-lg border {isMobile
								? ''
								: 'h-full'} flex flex-col overflow-hidden {isMobile
								? ''
								: 'sticky top-6'} max-h-[750px]"
						>
							<div
								class="flex items-center justify-between border-b bg-blue-50 p-4 dark:bg-blue-950"
							>
								<div>
									<div class="flex items-center gap-2">
										<span class="text-base font-medium text-blue-800 dark:text-blue-200"
											>📹 选择历史投稿</span
										>
										<span class="text-xs text-blue-600 dark:text-blue-400"
											>选择您希望下载的历史投稿。未选择的视频不会下载和显示。新发布的投稿会自动下载。</span
										>
									</div>
									<span
										class="text-sm text-blue-600 dark:text-blue-400 {isMobile
											? 'block'
											: 'ml-2'} mt-1"
									>
										{#if submissionLoading && submissionVideos.length === 0}
											正在加载...
										{:else if submissionTotalCount > 0}
											共 {submissionTotalCount} 个投稿
										{:else}
											暂无投稿
										{/if}
									</span>
								</div>
								<button
									onclick={cancelSubmissionSelection}
									class="p-1 text-xl text-blue-500 hover:text-blue-700 dark:text-blue-300 dark:hover:text-blue-100"
								>
									<X class="h-5 w-5" />
								</button>
							</div>

							<div class="flex min-h-0 flex-1 flex-col overflow-hidden">
								{#if submissionError}
									<div class="m-3 rounded-lg border border-red-200 bg-red-50 p-4">
										<div class="flex items-center gap-2">
											<svg
												class="h-5 w-5 text-red-600"
												fill="none"
												stroke="currentColor"
												viewBox="0 0 24 24"
											>
												<path
													stroke-linecap="round"
													stroke-linejoin="round"
													stroke-width="2"
													d="M12 8v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
												/>
											</svg>
											<span class="text-sm font-medium text-red-800 dark:text-red-200"
												>加载失败</span
											>
										</div>
										<p class="mt-1 text-sm text-red-700 dark:text-red-300">{submissionError}</p>
										<button
											type="button"
											class="mt-2 text-sm text-red-600 underline hover:text-red-800 dark:text-red-400 dark:hover:text-red-200"
											onclick={loadSubmissionVideos}
										>
											重试
										</button>
									</div>
								{:else}
									<!-- 搜索和操作栏 -->
									<div class="flex-shrink-0 space-y-3 p-3">
										<div class="flex gap-2">
											<div class="relative flex-1">
												<input
													type="text"
													bind:value={submissionSearchQuery}
													placeholder="搜索视频标题（支持关键词搜索UP主所有视频）..."
													class="w-full rounded-md border border-gray-300 px-3 py-2 pr-8 text-sm focus:border-blue-500 focus:ring-2 focus:ring-blue-500 focus:outline-none"
													disabled={isSearching}
												/>
												{#if isSearching}
													<div class="absolute inset-y-0 right-0 flex items-center pr-3">
														<svg
															class="h-4 w-4 animate-spin text-blue-600"
															fill="none"
															viewBox="0 0 24 24"
														>
															<circle
																class="opacity-25"
																cx="12"
																cy="12"
																r="10"
																stroke="currentColor"
																stroke-width="4"
															></circle>
															<path
																class="opacity-75"
																fill="currentColor"
																d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"
															></path>
														</svg>
													</div>
												{/if}
											</div>
										</div>

										{#if submissionSearchQuery.trim()}
											<div class="px-1 text-xs text-blue-600">
												{isSearching
													? '搜索中...'
													: `搜索模式：在UP主所有视频中搜索 "${submissionSearchQuery}"`}
											</div>
										{/if}

										<div class="flex items-center justify-between">
											<div class="flex gap-2">
												<button
													type="button"
													class="bg-card text-foreground hover:bg-muted rounded-md border border-gray-300 px-3 py-1 text-sm font-medium"
													onclick={selectAllSubmissions}
													disabled={filteredSubmissionVideos.length === 0}
												>
													全选
												</button>
												<button
													type="button"
													class="bg-card text-foreground hover:bg-muted rounded-md border border-gray-300 px-3 py-1 text-sm font-medium"
													onclick={selectNoneSubmissions}
													disabled={selectedSubmissionCount === 0}
												>
													全不选
												</button>
												<button
													type="button"
													class="bg-card text-foreground hover:bg-muted rounded-md border border-gray-300 px-3 py-1 text-sm font-medium"
													onclick={invertSubmissionSelection}
													disabled={filteredSubmissionVideos.length === 0}
												>
													反选
												</button>
											</div>

											<div class="text-muted-foreground text-sm">
												已选择 {selectedSubmissionCount} / {filteredSubmissionVideos.length} 个视频
											</div>
										</div>
									</div>

									<!-- 视频列表 -->
									<div
										class="min-h-0 flex-1 overflow-y-auto p-3 pt-0"
										bind:this={submissionScrollContainer}
										onscroll={handleSubmissionScroll}
									>
										{#if submissionLoading && submissionVideos.length === 0}
											<div class="flex items-center justify-center py-8">
												<svg
													class="h-8 w-8 animate-spin text-blue-600 dark:text-blue-400"
													fill="none"
													viewBox="0 0 24 24"
												>
													<circle
														class="opacity-25"
														cx="12"
														cy="12"
														r="10"
														stroke="currentColor"
														stroke-width="4"
													></circle>
													<path
														class="opacity-75"
														fill="currentColor"
														d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"
													></path>
												</svg>
												<span class="text-muted-foreground ml-2 text-sm">加载中...</span>
											</div>
										{:else if filteredSubmissionVideos.length === 0}
											<div
												class="text-muted-foreground flex flex-col items-center justify-center py-8"
											>
												<svg
													class="mb-2 h-12 w-12"
													fill="none"
													stroke="currentColor"
													viewBox="0 0 24 24"
												>
													<path
														stroke-linecap="round"
														stroke-linejoin="round"
														stroke-width="2"
														d="M19 11H5m14 0a2 2 0 012 2v6a2 2 0 01-2 2H5a2 2 0 01-2-2v-6a2 2 0 012-2m14 0V9a2 2 0 00-2-2M5 11V9a2 2 0 012-2m0 0V5a2 2 0 012-2h6a2 2 0 012 2v2M7 7h10"
													/>
												</svg>
												<p class="text-sm">没有找到视频</p>
											</div>
										{:else}
											<div
												class="grid gap-4 {isMobile ? 'grid-cols-1' : ''}"
												style={isMobile
													? ''
													: 'grid-template-columns: repeat(auto-fit, minmax(320px, 1fr));'}
											>
												{#each filteredSubmissionVideos as video (video.bvid)}
													<div
														class="hover:bg-muted relative rounded-lg border p-4 transition-all duration-300 hover:shadow-md {selectedSubmissionVideos.has(
															video.bvid
														)
															? 'border-blue-300 bg-blue-50'
															: 'border-gray-200'} {isMobile ? 'h-auto' : 'h-[100px]'}"
													>
														<div class="flex h-full gap-3">
															<div class="relative flex-shrink-0">
																<img
																	src={processBilibiliImageUrl(video.cover)}
																	alt={video.title}
																	class="h-[63px] w-28 rounded object-cover"
																	loading="lazy"
																	crossorigin="anonymous"
																	referrerpolicy="no-referrer"
																	onerror={handleImageError}
																/>
															</div>
															<div class="relative flex min-w-0 flex-1 flex-col overflow-hidden">
																<input
																	type="checkbox"
																	checked={selectedSubmissionVideos.has(video.bvid)}
																	onchange={() => toggleSubmissionVideo(video.bvid)}
																	class="absolute top-1 right-1 z-10 h-4 w-4 rounded border-gray-300 text-blue-600 focus:ring-blue-500 dark:text-blue-400"
																/>
																<h4
																	class="mb-1 line-clamp-2 flex-shrink-0 pr-6 text-sm font-medium text-gray-900"
																>
																	{video.title}
																</h4>
																<p
																	class="text-muted-foreground mb-2 line-clamp-1 flex-shrink-0 text-xs"
																>
																	{video.description || '无简介'}
																</p>
																<div class="text-muted-foreground mt-auto text-xs">
																	<div class="flex flex-wrap items-center gap-2">
																		<span>🎬 {formatSubmissionPlayCount(video.view)}</span>
																		<span>💬 {formatSubmissionPlayCount(video.danmaku)}</span>
																		<span>📅 {formatSubmissionDate(video.pubtime)}</span>
																		<span class="font-mono text-xs">{video.bvid}</span>
																	</div>
																</div>
															</div>
														</div>
													</div>
												{/each}
											</div>

											{#if submissionVideos.length > 0}
												{#if showLoadMoreButton && hasMoreVideos}
													<div class="py-4 text-center">
														<button
															type="button"
															class="rounded-md border border-transparent bg-blue-600 px-6 py-2 text-sm font-medium text-white hover:bg-blue-700 focus:ring-2 focus:ring-blue-500 focus:ring-offset-2 focus:outline-none disabled:cursor-not-allowed disabled:opacity-50"
															onclick={loadMoreSubmissionVideos}
															disabled={isLoadingMore}
														>
															{#if isLoadingMore}
																<div class="flex items-center gap-2">
																	<div
																		class="h-4 w-4 animate-spin rounded-full border-2 border-white border-t-transparent"
																	></div>
																	<span>加载中...</span>
																</div>
															{:else}
																加载更多 ({submissionVideos.length}/{submissionTotalCount})
															{/if}
														</button>
													</div>
												{:else if submissionTotalCount > 0}
													<div class="text-muted-foreground py-4 text-center text-sm">
														已加载全部 {submissionTotalCount} 个视频
													</div>
												{/if}
											{/if}
										{/if}
									</div>

									<!-- 确认按钮 -->
									<div class="flex flex-shrink-0 justify-end gap-3 border-t p-4">
										<button
											type="button"
											class="bg-card text-foreground hover:bg-muted rounded-md border border-gray-300 px-4 py-2 text-sm font-medium focus:ring-2 focus:ring-indigo-500 focus:ring-offset-2 focus:outline-none"
											onclick={cancelSubmissionSelection}
										>
											取消
										</button>
										<button
											type="button"
											class="rounded-md border border-transparent bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700 focus:ring-2 focus:ring-blue-500 focus:ring-offset-2 focus:outline-none"
											onclick={confirmSubmissionSelection}
										>
											确认选择 ({selectedSubmissionVideos.size} 个视频)
										</button>
									</div>
								{/if}
							</div>
						</div>
					</div>
				{/if}
			</div>
		</div>
	</div>
</div>

<!-- 批量操作工具栏 -->
{#if batchMode && batchSelectedItems.size > 0}
	<div
		class="fixed {isMobile
			? 'right-4 bottom-4 left-4'
			: 'bottom-6 left-1/2 -translate-x-1/2'} z-50 rounded-lg border border-blue-500 bg-blue-600 px-4 py-3 text-white shadow-xl transition-all duration-300 dark:border-blue-600 dark:bg-blue-700"
		transition:fly={{ y: 100, duration: 300 }}
	>
		<div class="flex {isMobile ? 'flex-col gap-3' : 'items-center gap-4'}">
			<div class="text-sm font-medium {isMobile ? 'text-center' : ''}">
				已选择 {batchSelectedItems.size} 个视频源
			</div>
			<div class="flex gap-2 {isMobile ? 'justify-center' : ''}">
				<Button
					size="sm"
					variant="secondary"
					onclick={clearBatchSelection}
					class="border-white/30 bg-white/20 text-xs text-white hover:bg-white/30"
				>
					清空
				</Button>
				<Button
					size="sm"
					variant="secondary"
					onclick={() => {
						batchDialogOpen = true;
					}}
					disabled={batchAdding}
					class="bg-white text-xs text-blue-600 hover:bg-gray-100 dark:text-blue-700"
				>
					{batchAdding ? '添加中...' : '批量添加'}
				</Button>
			</div>
		</div>
		{#if batchAdding}
			<div class="mt-2 text-xs text-blue-100 dark:text-blue-200 {isMobile ? 'text-center' : ''}">
				正在添加 ({batchProgress.current}/{batchProgress.total})
			</div>
		{/if}
	</div>
{/if}

<!-- 统一的悬停详情框 -->
{#if hoveredItem}
	<div
		class="bg-card pointer-events-none fixed z-50 max-w-md rounded-lg border p-4 shadow-2xl transition-all duration-150 ease-out"
		style="left: {mousePosition.x}px; top: {mousePosition.y}px;"
		transition:fade={{ duration: 200 }}
	>
		{#if hoveredItem.type === 'search'}
			<!-- 搜索结果详情内容 -->
			<div class="flex gap-4">
				{#if hoveredItem.data.cover}
					<img
						src={processBilibiliImageUrl(hoveredItem.data.cover)}
						alt={hoveredItem.data.title}
						class="{sourceType === 'bangumi'
							? 'h-32 w-24'
							: 'h-20 w-32'} flex-shrink-0 rounded object-cover"
						loading="lazy"
						crossorigin="anonymous"
						referrerpolicy="no-referrer"
					/>
				{:else}
					<div
						class="{sourceType === 'bangumi'
							? 'h-32 w-24'
							: 'h-20 w-32'} bg-muted text-muted-foreground flex flex-shrink-0 items-center justify-center rounded text-sm"
					>
						无图片
					</div>
				{/if}
				<div class="min-w-0 flex-1">
					<div class="mb-1 flex items-center gap-2">
						<h4 class="flex-1 text-sm font-semibold">
							<!-- eslint-disable-next-line svelte/no-at-html-tags -->
							{@html hoveredItem.data.title}
						</h4>
						{#if hoveredItem.data.result_type}
							<span
								class="flex-shrink-0 rounded px-1.5 py-0.5 text-xs {hoveredItem.data.result_type ===
								'media_bangumi'
									? 'bg-purple-100 text-purple-700 dark:bg-purple-900 dark:text-purple-300'
									: hoveredItem.data.result_type === 'media_ft'
										? 'bg-red-100 text-red-700'
										: hoveredItem.data.result_type === 'bili_user'
											? 'bg-blue-100 text-blue-700'
											: hoveredItem.data.result_type === 'video'
												? 'bg-green-100 text-green-700 dark:bg-green-900 dark:text-green-300'
												: 'text-foreground bg-gray-100'}"
							>
								{hoveredItem.data.result_type === 'media_bangumi'
									? '番剧'
									: hoveredItem.data.result_type === 'media_ft'
										? '影视'
										: hoveredItem.data.result_type === 'bili_user'
											? 'UP主'
											: hoveredItem.data.result_type === 'video'
												? '视频'
												: hoveredItem.data.result_type}
							</span>
						{/if}
					</div>
					<p class="text-muted-foreground mb-2 text-xs">作者：{hoveredItem.data.author}</p>
					{#if hoveredItem.data.description}
						<p class="text-muted-foreground mb-2 line-clamp-4 text-xs">
							{hoveredItem.data.description}
						</p>
					{/if}
					<div class="flex flex-wrap gap-2 text-xs">
						{#if hoveredItem.data.play}
							<span class="text-muted-foreground flex items-center gap-1">
								<span>▶</span> 播放：{hoveredItem.data.play > 10000
									? (hoveredItem.data.play / 10000).toFixed(1) + '万'
									: hoveredItem.data.play}
							</span>
						{/if}
						{#if hoveredItem.data.danmaku}
							<span class="text-muted-foreground flex items-center gap-1">
								<span>💬</span> 弹幕：{hoveredItem.data.danmaku > 10000
									? (hoveredItem.data.danmaku / 10000).toFixed(1) + '万'
									: hoveredItem.data.danmaku}
							</span>
						{/if}
						{#if sourceType === 'bangumi' && hoveredItem.data.season_id}
							<span class="text-muted-foreground">Season ID: {hoveredItem.data.season_id}</span>
						{/if}
						{#if hoveredItem.data.bvid}
							<span class="text-muted-foreground">BV号: {hoveredItem.data.bvid}</span>
						{/if}
					</div>
				</div>
			</div>
		{:else if hoveredItem.type === 'season'}
			<!-- 季度选择详情内容 -->
			<div class="flex gap-4">
				{#if hoveredItem.data.cover}
					<img
						src={processBilibiliImageUrl(hoveredItem.data.cover)}
						alt={hoveredItem.data.season_title || hoveredItem.data.title}
						class="h-32 w-24 flex-shrink-0 rounded object-cover"
						loading="lazy"
						crossorigin="anonymous"
						referrerpolicy="no-referrer"
					/>
				{:else}
					<div
						class="bg-muted text-muted-foreground flex h-32 w-24 flex-shrink-0 items-center justify-center rounded text-sm"
					>
						无封面
					</div>
				{/if}
				<div class="min-w-0 flex-1">
					<div class="mb-1 flex items-center gap-2">
						<h4 class="flex-1 text-sm font-semibold">
							{hoveredItem.data.full_title ||
								hoveredItem.data.season_title ||
								hoveredItem.data.title}
						</h4>
						<span
							class="flex-shrink-0 rounded bg-purple-100 px-1.5 py-0.5 text-xs text-purple-700 dark:bg-purple-900 dark:text-purple-300"
						>
							番剧
						</span>
					</div>

					<div class="space-y-2 text-xs">
						{#if hoveredItem.data.description}
							<div class="text-foreground mb-3 line-clamp-3 text-sm leading-relaxed">
								{hoveredItem.data.description}
							</div>
						{/if}

						<div class="flex flex-wrap gap-3">
							<span class="text-muted-foreground"
								>Season ID: <span class="font-mono text-gray-800 dark:text-gray-200"
									>{hoveredItem.data.season_id}</span
								></span
							>
							{#if hoveredItem.data.media_id}
								<span class="text-muted-foreground"
									>Media ID: <span class="font-mono text-gray-800 dark:text-gray-200"
										>{hoveredItem.data.media_id}</span
									></span
								>
							{/if}
						</div>

						{#if hoveredItem.data.episode_count}
							<div class="text-muted-foreground flex items-center gap-1">
								<span>📺</span> 总集数：{hoveredItem.data.episode_count} 集
							</div>
						{/if}

						{#if hoveredItem.data.season_id === sourceId}
							<div class="font-medium text-purple-600">🎯 当前选择的季度</div>
						{/if}

						{#if selectedSeasons.includes(hoveredItem.data.season_id)}
							<div class="font-medium text-green-600">✅ 已选择下载</div>
						{/if}
					</div>
				</div>
			</div>
		{/if}
	</div>
{/if}

<!-- 批量添加配置对话框 -->
{#if batchDialogOpen}
	<div
		class="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm"
		transition:fade
	>
		<div
			class="bg-card mx-4 w-full max-w-md rounded-lg border shadow-lg"
			transition:fly={{ y: -50 }}
		>
			<div class="border-b p-4">
				<h3 class="text-lg font-semibold">批量添加配置</h3>
				<p class="text-muted-foreground mt-1 text-sm">
					将添加 {batchSelectedItems.size} 个视频源
				</p>
			</div>

			<div class="space-y-4 p-4">
				<div>
					<Label for="batch-base-path">基础保存路径</Label>
					<Input
						id="batch-base-path"
						bind:value={batchBasePath}
						placeholder="/Downloads"
						class="mt-1"
					/>
					<p class="text-muted-foreground mt-1 text-xs">所有选中的视频源将保存到此路径</p>
				</div>

				<div class="max-h-60 overflow-y-auto rounded border">
					<div class="space-y-2 p-3">
						{#each Array.from(batchSelectedItems.values()) as item, index}
							<div class="bg-muted flex items-center justify-between rounded p-2 text-sm">
								<div class="min-w-0 flex-1">
									<div class="truncate font-medium">{item.name}</div>
									<div class="text-muted-foreground truncate text-xs">
										{batchBasePath}
									</div>
								</div>
								<span class="bg-background ml-2 rounded px-2 py-1 text-xs">
									{item.type === 'search'
										? '搜索'
										: item.type === 'favorite'
											? '收藏夹'
											: item.type === 'following'
												? 'UP主'
												: item.type === 'bangumi'
													? '番剧'
													: item.type}
								</span>
							</div>
						{/each}
					</div>
				</div>
			</div>

			<div class="flex justify-end gap-2 border-t p-4">
				<Button
					variant="outline"
					onclick={() => {
						batchDialogOpen = false;
					}}
					disabled={batchAdding}
				>
					取消
				</Button>
				<Button onclick={handleBatchAdd} disabled={batchAdding || !batchBasePath.trim()}>
					{batchAdding ? '添加中...' : '开始添加'}
				</Button>
			</div>
		</div>
	</div>
{/if}

<style>
	/* 确保图片加载失败时的占位符正确显示 */
	:global(.placeholder) {
		flex-shrink: 0;
	}

	/* 限制描述文字的行数 */
	.line-clamp-2 {
		display: -webkit-box;
		-webkit-line-clamp: 2;
		-webkit-box-orient: vertical;
		overflow: hidden;
	}

	.line-clamp-3 {
		display: -webkit-box;
		-webkit-line-clamp: 3;
		-webkit-box-orient: vertical;
		overflow: hidden;
	}

	.line-clamp-4 {
		display: -webkit-box;
		-webkit-line-clamp: 4;
		-webkit-box-orient: vertical;
		overflow: hidden;
	}

	/* 悬停动画效果 */
	.hover\:scale-102:hover {
		transform: scale(1.02);
	}

	.transform {
		transition:
			transform 0.3s ease,
			box-shadow 0.3s ease;
	}

	/* 季度网格容器滚动样式 */
	.seasons-grid-container {
		max-height: calc(120px * 5 + 1rem * 4); /* 5个横向行，每行120px高度，4个行间隔 */
		overflow-y: auto;
		padding-right: 0.5rem;
	}

	.seasons-grid-container::-webkit-scrollbar {
		width: 6px;
	}

	.seasons-grid-container::-webkit-scrollbar-track {
		background: #f1f1f1;
		border-radius: 3px;
	}

	.seasons-grid-container::-webkit-scrollbar-thumb {
		background: #c1c1c1;
		border-radius: 3px;
	}

	.seasons-grid-container::-webkit-scrollbar-thumb:hover {
		background: #a1a1a1;
	}
</style>
