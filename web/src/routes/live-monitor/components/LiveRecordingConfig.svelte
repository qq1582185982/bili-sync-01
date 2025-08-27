<script lang="ts">
	import { createEventDispatcher } from 'svelte';
	import * as Card from '$lib/components/ui/card';
	import * as Tabs from '$lib/components/ui/tabs';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Badge } from '$lib/components/ui/badge';
	import { toast } from 'svelte-sonner';
	import api from '$lib/api';
	import { onMount } from 'svelte';
	import Loading from '$lib/components/ui/Loading.svelte';
	import type { RecordingMode } from '$lib/types';

	const dispatch = createEventDispatcher<{
		close: void;
	}>();

	// 配置数据
	let config = {
		recording_mode: 'ffmpeg' as RecordingMode,
		auto_merge: {
			enabled: false,
			duration_threshold: 600,
			keep_segments_after_merge: false,
			output_format: 'mp4',
			output_quality: 'Auto'
		},
		quality: {
			preferred_format: 'flv',
			quality_level: 10000, // 默认原画
			frame_rate: 30
		},
		file_management: {
			max_segments_to_keep: 50,
			filename_template: '{upper_name}_{room_id}_{date}_{time}_{title}.{ext}',
			auto_cleanup_days: 7
		}
	};

	let loading = false;
	let saving = false;

	// 下拉选项
	const formatOptions = [
		{ value: 'mp4', label: 'MP4' },
		{ value: 'mkv', label: 'MKV' }, 
		{ value: 'flv', label: 'FLV' }
	];

	const qualityOptions = [
		{ value: 'StreamCopy', label: '流复制（无损，速度快）' },
		{ value: 'Reencode', label: '重编码（有损，文件小）' },
		{ value: 'Auto', label: '自动选择' }
	];

	const recordingModeOptions = [
		{ value: 'ffmpeg', label: 'FFmpeg模式', description: '直接录制到文件，适合FLV/MP4格式' },
		{ value: 'segment', label: '分片模式', description: 'HLS分片下载并合并，适合M4S格式' }
	];

	const recordingFormatOptions = [
		{ value: 'flv', label: 'FLV' },
		{ value: 'm4s', label: 'M4S (HLS)' }
	];

	// 质量等级选项
	let qualityLevelOptions = [
		{ value: 10000, label: '原画', description: '最高画质，原始分辨率' },
		{ value: 800, label: '4K', description: '4K超高清画质' },
		{ value: 401, label: '蓝光杜比', description: '蓝光画质，支持杜比音效' },
		{ value: 400, label: '蓝光', description: '蓝光画质' },
		{ value: 250, label: '超清', description: '超清画质，通常为720p或1080p' },
		{ value: 150, label: '高清', description: '高清画质，通常为720p' },
		{ value: 80, label: '流畅', description: '流畅画质，通常为480p' }
	];

	// 加载配置
	async function loadConfig() {
		loading = true;
		try {
			// 并行加载配置和质量等级选项
			const [configData, qualityLevels] = await Promise.all([
				api.getLiveRecordingConfig(),
				api.getLiveQualityLevels().catch(() => null) // 如果获取失败，使用默认选项
			]);
			
			config = configData;
			
			// 如果成功获取到质量等级，则使用API数据
			if (qualityLevels && qualityLevels.length > 0) {
				qualityLevelOptions = qualityLevels.map(q => ({
					value: q.qn,
					label: q.name,
					description: q.description
				}));
			}
		} catch (error) {
			console.error('加载录制配置失败:', error);
			toast.error('加载配置失败');
		} finally {
			loading = false;
		}
	}

	// 保存配置
	async function saveConfig() {
		if (saving) return;
		
		saving = true;
		try {
			await api.updateLiveRecordingConfig(config);
			toast.success('配置保存成功');
			dispatch('close');
		} catch (error) {
			console.error('保存录制配置失败:', error);
			toast.error('保存配置失败');
		} finally {
			saving = false;
		}
	}

	// 关闭对话框
	function handleClose() {
		dispatch('close');
	}

	// 智能模式推荐：当选择M4S格式时推荐分片模式
	$: {
		if (config.quality.preferred_format === 'm4s' && config.recording_mode === 'ffmpeg') {
			// 可以在这里添加提示逻辑，但不自动改变用户选择
		}
	}

	// 组件挂载时加载配置
	onMount(() => {
		loadConfig();
	});
</script>

<div class="fixed inset-0 z-[9999] bg-black/80" role="presentation" on:click={handleClose}>
	<div class="fixed left-[50%] top-[50%] z-[10000] grid w-full max-w-4xl translate-x-[-50%] translate-y-[-50%] gap-4 border bg-background p-6 shadow-lg duration-200 rounded-lg max-h-[90vh] overflow-y-auto" 
		 on:click|stopPropagation 
		 on:keydown={(e) => e.key === 'Escape' && handleClose()} 
		 role="dialog" 
		 tabindex="-1">
		
		<!-- 对话框标题 -->
		<div class="flex flex-col space-y-1.5 text-center sm:text-left">
			<h2 class="text-lg font-semibold leading-none tracking-tight">
				直播录制配置
			</h2>
			<p class="text-sm text-muted-foreground">
				配置自动合并、录制质量和文件管理选项
			</p>
		</div>

		{#if loading}
			<div class="flex justify-center p-8">
				<Loading />
			</div>
		{:else}
			<Tabs.Root value="auto-merge" class="w-full">
				<Tabs.List class="grid w-full grid-cols-3">
					<Tabs.Trigger value="auto-merge">自动合并</Tabs.Trigger>
					<Tabs.Trigger value="quality">录制质量</Tabs.Trigger>
					<Tabs.Trigger value="file-management">文件管理</Tabs.Trigger>
				</Tabs.List>

				<!-- 自动合并配置 -->
				<Tabs.Content value="auto-merge" class="space-y-4">
					<Card.Root>
						<Card.Header>
							<Card.Title>自动合并配置</Card.Title>
							<Card.Description>当录制时长达到阈值时自动合并分片文件</Card.Description>
						</Card.Header>
						<Card.Content class="space-y-4">
							<!-- 启用自动合并 -->
							<div class="flex items-center space-x-2">
								<input
									type="checkbox"
									id="auto-merge-enabled"
									bind:checked={config.auto_merge.enabled}
									class="text-primary focus:ring-primary h-4 w-4 rounded border-gray-300"
								/>
								<div>
									<Label for="auto-merge-enabled" class="text-base font-medium">启用自动合并</Label>
									<p class="text-sm text-muted-foreground">启用后会在达到时长阈值时自动合并分片</p>
								</div>
							</div>

							<!-- 时长阈值 -->
							<div class="space-y-2">
								<Label for="duration-threshold">时长阈值（秒）</Label>
								<div class="flex items-center space-x-4">
									<input
										id="duration-threshold"
										type="range"
										min="60"
										max="3600"
										step="60"
										bind:value={config.auto_merge.duration_threshold}
										class="flex-1"
									/>
									<Badge variant="outline" class="text-sm min-w-fit">
										{Math.floor(config.auto_merge.duration_threshold / 60)}分钟
									</Badge>
								</div>
								<p class="text-sm text-muted-foreground">当录制时长达到此阈值时触发自动合并</p>
							</div>

							<!-- 保留分片文件 -->
							<div class="flex items-center space-x-2">
								<input
									type="checkbox"
									id="keep-segments"
									bind:checked={config.auto_merge.keep_segments_after_merge}
									class="text-primary focus:ring-primary h-4 w-4 rounded border-gray-300"
								/>
								<div>
									<Label for="keep-segments" class="text-base font-medium">合并后保留分片文件</Label>
									<p class="text-sm text-muted-foreground">合并完成后是否保留原始分片文件</p>
								</div>
							</div>

							<!-- 输出格式 -->
							<div class="space-y-2">
								<Label for="output-format">输出格式</Label>
								<select
									id="output-format"
									bind:value={config.auto_merge.output_format}
									class="border-input bg-background ring-offset-background focus-visible:ring-ring flex h-10 w-full rounded-md border px-3 py-2 text-sm focus-visible:ring-2 focus-visible:ring-offset-2 focus-visible:outline-none"
								>
									{#each formatOptions as option}
										<option value={option.value}>{option.label}</option>
									{/each}
								</select>
							</div>

							<!-- 合并质量 -->
							<div class="space-y-2">
								<Label for="merge-quality">合并质量</Label>
								<select
									id="merge-quality"
									bind:value={config.auto_merge.output_quality}
									class="border-input bg-background ring-offset-background focus-visible:ring-ring flex h-10 w-full rounded-md border px-3 py-2 text-sm focus-visible:ring-2 focus-visible:ring-offset-2 focus-visible:outline-none"
								>
									{#each qualityOptions as option}
										<option value={option.value}>{option.label}</option>
									{/each}
								</select>
							</div>
						</Card.Content>
					</Card.Root>
				</Tabs.Content>

				<!-- 录制质量配置 -->
				<Tabs.Content value="quality" class="space-y-4">
					<Card.Root>
						<Card.Header>
							<Card.Title>录制质量配置</Card.Title>
							<Card.Description>设置录制流的格式、分辨率和帧率</Card.Description>
						</Card.Header>
						<Card.Content class="space-y-4">
							<!-- 录制模式 -->
							<div class="space-y-2">
								<Label for="recording-mode">录制模式</Label>
								<select
									id="recording-mode"
									bind:value={config.recording_mode}
									class="border-input bg-background ring-offset-background focus-visible:ring-ring flex h-10 w-full rounded-md border px-3 py-2 text-sm focus-visible:ring-2 focus-visible:ring-offset-2 focus-visible:outline-none"
								>
									{#each recordingModeOptions as option}
										<option value={option.value}>{option.label}</option>
									{/each}
								</select>
								<p class="text-sm text-muted-foreground">
									{#if config.recording_mode}
										{recordingModeOptions.find(m => m.value === config.recording_mode)?.description || ''}
									{:else}
										选择录制模式
									{/if}
								</p>
							</div>

							<!-- 首选格式 -->
							<div class="space-y-2">
								<Label for="preferred-format">首选录制格式</Label>
								<select
									id="preferred-format"
									bind:value={config.quality.preferred_format}
									class="border-input bg-background ring-offset-background focus-visible:ring-ring flex h-10 w-full rounded-md border px-3 py-2 text-sm focus-visible:ring-2 focus-visible:ring-offset-2 focus-visible:outline-none"
								>
									{#each recordingFormatOptions as option}
										<option value={option.value}>{option.label}</option>
									{/each}
								</select>
								{#if config.quality.preferred_format === 'm4s' && config.recording_mode === 'ffmpeg'}
									<div class="flex items-start space-x-2 mt-2 p-3 bg-yellow-50 border border-yellow-200 rounded-md dark:bg-yellow-900/20 dark:border-yellow-800">
										<div class="flex-shrink-0">
											<svg class="h-5 w-5 text-yellow-400" viewBox="0 0 20 20" fill="currentColor">
												<path fill-rule="evenodd" d="M8.485 3.495c.673-1.167 2.357-1.167 3.03 0l6.28 10.875c.673 1.167-.17 2.625-1.516 2.625H3.72c-1.347 0-2.189-1.458-1.515-2.625L8.485 3.495zM10 6a.75.75 0 01.75.75v3.5a.75.75 0 01-1.5 0v-3.5A.75.75 0 0110 6zm0 9a1 1 0 100-2 1 1 0 000 2z" clip-rule="evenodd" />
											</svg>
										</div>
										<div>
											<h3 class="text-sm font-medium text-yellow-800 dark:text-yellow-200">建议使用分片模式</h3>
											<p class="text-sm text-yellow-700 dark:text-yellow-300 mt-1">M4S格式建议使用分片模式以支持自动合并为MP4文件</p>
										</div>
									</div>
								{/if}
							</div>

							<!-- 质量等级 -->
							<div class="space-y-2">
								<Label for="quality-level">录制质量等级</Label>
								<select
									id="quality-level"
									bind:value={config.quality.quality_level}
									class="border-input bg-background ring-offset-background focus-visible:ring-ring flex h-10 w-full rounded-md border px-3 py-2 text-sm focus-visible:ring-2 focus-visible:ring-offset-2 focus-visible:outline-none"
								>
									{#each qualityLevelOptions as option}
										<option value={option.value}>{option.label}</option>
									{/each}
								</select>
								<p class="text-sm text-muted-foreground">
									{#if config.quality.quality_level}
										{qualityLevelOptions.find(q => q.value === config.quality.quality_level)?.description || '自定义质量等级'}
									{:else}
										选择录制质量等级，数值越高画质越好
									{/if}
								</p>
							</div>

							<!-- 帧率 -->
							<div class="space-y-2">
								<Label for="frame-rate">录制帧率 (fps)</Label>
								<Input
									id="frame-rate"
									type="number"
									min="15"
									max="60"
									bind:value={config.quality.frame_rate}
									placeholder="30"
								/>
								<p class="text-sm text-muted-foreground">推荐值：30fps，高动作内容可设置60fps</p>
							</div>
						</Card.Content>
					</Card.Root>
				</Tabs.Content>

				<!-- 文件管理配置 -->
				<Tabs.Content value="file-management" class="space-y-4">
					<Card.Root>
						<Card.Header>
							<Card.Title>文件管理配置</Card.Title>
							<Card.Description>配置分片保留、文件命名和自动清理选项</Card.Description>
						</Card.Header>
						<Card.Content class="space-y-4">
							<!-- 分片保留数量 -->
							<div class="space-y-2">
								<Label for="max-segments">分片文件保留数量</Label>
								<div class="flex items-center space-x-4">
									<input
										id="max-segments"
										type="range"
										min="10"
										max="100"
										step="10"
										bind:value={config.file_management.max_segments_to_keep}
										class="flex-1"
									/>
									<Badge variant="outline" class="text-sm min-w-fit">
										{config.file_management.max_segments_to_keep}个
									</Badge>
								</div>
								<p class="text-sm text-muted-foreground">超过此数量的旧分片文件将被自动删除</p>
							</div>

							<!-- 文件命名模板 -->
							<div class="space-y-2">
								<Label for="filename-template">文件命名模板</Label>
								<Input
									id="filename-template"
									bind:value={config.file_management.filename_template}
									placeholder={"{upper_name}_{room_id}_{date}_{time}_{title}.{ext}"}
								/>
								<div class="text-xs text-muted-foreground space-y-1">
									<p><strong>可用变量：</strong></p>
									<p>&#123;upper_name&#125; - UP主名称, &#123;room_id&#125; - 直播间ID, &#123;date&#125; - 录制日期</p>
									<p>&#123;time&#125; - 录制时间, &#123;title&#125; - 直播标题, &#123;ext&#125; - 文件扩展名</p>
								</div>
							</div>

							<!-- 自动清理天数 -->
							<div class="space-y-2">
								<Label for="cleanup-days">自动清理天数</Label>
								<div class="flex items-center space-x-4">
									<input
										id="cleanup-days"
										type="range"
										min="1"
										max="30"
										step="1"
										bind:value={config.file_management.auto_cleanup_days}
										class="flex-1"
									/>
									<Badge variant="outline" class="text-sm min-w-fit">
										{config.file_management.auto_cleanup_days}天
									</Badge>
								</div>
								<p class="text-sm text-muted-foreground">超过此天数的录制文件将被自动删除</p>
							</div>
						</Card.Content>
					</Card.Root>
				</Tabs.Content>
			</Tabs.Root>

			<!-- 操作按钮 -->
			<div class="flex justify-end space-x-2">
				<button 
					type="button"
					on:click={handleClose}
					class="inline-flex items-center justify-center whitespace-nowrap rounded-md text-sm font-medium ring-offset-background transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:pointer-events-none disabled:opacity-50 border border-input bg-background hover:bg-accent hover:text-accent-foreground h-10 px-4 py-2"
				>
					取消
				</button>
				<button 
					type="button"
					on:click={saveConfig} 
					disabled={saving}
					class="inline-flex items-center justify-center whitespace-nowrap rounded-md text-sm font-medium ring-offset-background transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:pointer-events-none disabled:opacity-50 bg-primary text-primary-foreground hover:bg-primary/90 h-10 px-4 py-2"
				>
					{#if saving}
						保存中...
					{:else}
						保存配置
					{/if}
				</button>
			</div>
		{/if}
	</div>
</div>