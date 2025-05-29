<script lang="ts">
	import { Input } from '$lib/components/ui/input';
	import { Button } from '$lib/components/ui/button';
	import { getConfig, updateConfig } from '$lib/api';
	import { toast } from 'svelte-sonner';
	import { onMount } from 'svelte';

	export let onSuccess: () => void;

	let video_name = '{{' + 'title' + '}}';
	let page_name = '{{' + 'title' + '}}';
	let multi_page_name = '{{' + 'title' + '}}-P{{' + 'pid_pad' + '}}';
	let bangumi_name = 'S{{' + 'season_pad' + '}}E{{' + 'pid_pad' + '}}-{{' + 'pid_pad' + '}}';
	let folder_structure = 'Season 1';
	let time_format = '%Y-%m-%d';
	let interval = 1200;
	let nfo_time_type = 'favtime';
	let loading = false;
	let loadingConfig = true;

	// 配置字段说明
	const fieldDescriptions = {
		video_name: '视频文件夹命名模板',
		page_name: '分页文件命名模板（单P视频使用）',
		multi_page_name: '多P视频分页命名模板（多P视频专用）',
		bangumi_name: '番剧文件命名模板（番剧专用）',
		folder_structure: '多页视频的文件夹结构模板',
		time_format: '时间格式',
		interval: '扫描间隔时间（秒），建议不少于60秒',
		nfo_time_type: 'NFO文件中使用的时间类型'
	};

	// 变量说明
	const variableHelp = {
		video: [
			{ name: '{{' + 'title' + '}}', desc: '视频标题' },
			{ name: '{{' + 'bvid' + '}}', desc: 'BV号（视频编号）' },
			{ name: '{{' + 'upper_name' + '}}', desc: 'UP主名称' },
			{ name: '{{' + 'upper_mid' + '}}', desc: 'UP主ID' },
			{ name: '{{' + 'pubtime' + '}}', desc: '视频发布时间' },
			{ name: '{{' + 'fav_time' + '}}', desc: '视频收藏时间（仅收藏夹视频有效）' }
		],
		page: [
			{ name: '{{' + 'ptitle' + '}}', desc: '分页标题' },
			{ name: '{{' + 'pid' + '}}', desc: '分页页号' },
			{ name: '{{' + 'pid_pad' + '}}', desc: '补零的分页页号（如001、002）' },
			{ name: '{{' + 'season_pad' + '}}', desc: '补零的季度号（多P视频默认为01）' }
		],
		common: [
			{ name: '{{' + ' truncate title 10 ' + '}}', desc: '截取函数示例：截取标题前10个字符' },
			{ name: '路径分隔符', desc: '支持使用 / 或 \\\\ 创建子文件夹' }
		],
		time: [
			{ name: '%Y', desc: '年份（如2023）' },
			{ name: '%m', desc: '月份（如01-12）' },
			{ name: '%d', desc: '日期（如01-31）' },
			{ name: '%H', desc: '小时（如00-23）' },
			{ name: '%M', desc: '分钟（如00-59）' },
			{ name: '%S', desc: '秒数（如00-59）' }
		]
	};

	let showHelp = false;

	// NFO时间类型选项
	const nfoTimeTypeOptions = [
		{ value: 'favtime', label: '收藏时间' },
		{ value: 'pubtime', label: '发布时间' }
	];

	// 加载当前配置
	async function loadConfig() {
		try {
			loadingConfig = true;
			const config = await getConfig();
			video_name = config.video_name;
			page_name = config.page_name;
			multi_page_name = config.multi_page_name || '{{' + 'title' + '}}-P{{' + 'pid_pad' + '}}';
			bangumi_name = config.bangumi_name || 'S{{' + 'season_pad' + '}}E{{' + 'pid_pad' + '}}-{{' + 'pid_pad' + '}}';
			folder_structure = config.folder_structure;
			time_format = config.time_format;
			interval = config.interval;
			nfo_time_type = config.nfo_time_type;
		} catch (error) {
			console.error('加载配置失败:', error);
			toast.error('加载配置失败', { description: `错误信息：${error}` });
		} finally {
			loadingConfig = false;
		}
	}

	async function handleSubmit() {
		// 基本验证
		if (!video_name.trim()) {
			toast.error('请输入视频命名模板', { description: '视频命名模板不能为空' });
			return;
		}
		
		if (!page_name.trim()) {
			toast.error('请输入分页命名模板', { description: '分页命名模板不能为空' });
			return;
		}
		
		if (!multi_page_name.trim()) {
			toast.error('请输入多P视频命名模板', { description: '多P视频命名模板不能为空' });
			return;
		}
		
		if (!bangumi_name.trim()) {
			toast.error('请输入番剧命名模板', { description: '番剧命名模板不能为空' });
			return;
		}
		
		if (!folder_structure.trim()) {
			toast.error('请输入文件夹结构模板', { description: '文件夹结构模板不能为空' });
			return;
		}
		
		if (!time_format.trim()) {
			toast.error('请输入时间格式', { description: '时间格式不能为空' });
			return;
		}
		
		if (interval < 60) {
			toast.error('扫描间隔过短', { description: '建议设置不少于60秒，避免频繁请求' });
			return;
		}
		
		loading = true;
		
		try {
			const result = await updateConfig({
				video_name: video_name.trim(),
				page_name: page_name.trim(),
				multi_page_name: multi_page_name.trim(),
				bangumi_name: bangumi_name.trim(),
				folder_structure: folder_structure.trim(),
				time_format: time_format.trim(),
				interval,
				nfo_time_type
			});
			
			if (result.success) {
				toast.success('配置更新成功', { 
					description: result.updated_files !== undefined 
						? `${result.message}，正在后台重命名已下载的文件` 
						: result.message 
				});
				onSuccess();
			} else {
				toast.error('配置更新失败', { description: result.message });
			}
		} catch (error) {
			console.error('更新配置失败:', error);
			toast.error('配置更新失败', { description: `错误信息：${error}` });
		} finally {
			loading = false;
		}
	}

	onMount(loadConfig);
</script>

<div class="bg-white p-4 rounded shadow-md">
	<div class="flex justify-between items-center mb-4">
		<h2 class="text-xl font-bold">配置管理</h2>
		<button 
			type="button" 
			class="px-3 py-1 text-sm bg-purple-500 hover:bg-purple-600 text-white border border-purple-500 rounded"
			on:click={() => {
				console.log('按钮被点击，当前showHelp:', showHelp);
				showHelp = !showHelp;
				console.log('更新后showHelp:', showHelp);
			}}
		>
			{showHelp ? '隐藏' : '显示'}变量说明
		</button>
	</div>
	
	{#if showHelp}
		<div class="mb-6 p-4 bg-gray-50 rounded border">
			<h3 class="text-lg font-semibold mb-3">📝 支持的模板变量</h3>
			
			<div class="grid grid-cols-1 md:grid-cols-2 gap-4">
				<div>
					<h4 class="font-medium text-blue-600 mb-2">🎬 视频变量</h4>
					<div class="space-y-1 text-sm">
						{#each variableHelp.video as variable}
							<div class="flex">
								<code class="bg-blue-100 px-2 py-1 rounded text-blue-800 mr-2 min-w-fit">{variable.name}</code>
								<span class="text-gray-600">{variable.desc}</span>
							</div>
						{/each}
					</div>
				</div>
				
				<div>
					<h4 class="font-medium text-green-600 mb-2">📄 分页变量</h4>
					<div class="space-y-1 text-sm">
						{#each variableHelp.page as variable}
							<div class="flex">
								<code class="bg-green-100 px-2 py-1 rounded text-green-800 mr-2 min-w-fit">{variable.name}</code>
								<span class="text-gray-600">{variable.desc}</span>
							</div>
						{/each}
					</div>
				</div>
				
				<div>
					<h4 class="font-medium text-purple-600 mb-2">🔧 通用功能</h4>
					<div class="space-y-1 text-sm">
						{#each variableHelp.common as variable}
							<div class="flex">
								<code class="bg-purple-100 px-2 py-1 rounded text-purple-800 mr-2 min-w-fit">{variable.name}</code>
								<span class="text-gray-600">{variable.desc}</span>
							</div>
						{/each}
					</div>
				</div>
				
				<div>
					<h4 class="font-medium text-orange-600 mb-2">⏰ 时间格式</h4>
					<div class="space-y-1 text-sm">
						{#each variableHelp.time as variable}
							<div class="flex">
								<code class="bg-orange-100 px-2 py-1 rounded text-orange-800 mr-2 min-w-fit">{variable.name}</code>
								<span class="text-gray-600">{variable.desc}</span>
							</div>
						{/each}
					</div>
				</div>
			</div>
			
			<div class="mt-4 p-3 bg-blue-50 rounded border-l-4 border-blue-400">
				<h4 class="font-medium text-blue-800 mb-2">💡 使用示例</h4>
				<div class="text-sm text-blue-700 space-y-2">
					<div>
						<strong>视频命名模板：</strong>
						<div class="ml-4 space-y-1">
							<div><code>{'{{upper_name}} - {{title}}'}</code> → <span class="text-gray-600">庄心妍 - 没想到吧～这些歌原来是我唱的！</span></div>
							<div><code>{'{{title}} [{{bvid}}]'}</code> → <span class="text-gray-600">【觅长生】废人修仙传#01 修真界来个废物 [BV1abc123def]</span></div>
							<div><code>{'{{upper_name}}/{{title}}_{{pubtime}}'}</code> → <span class="text-gray-600">庄心妍/庄心妍的街头采访_2023-12-25</span></div>
						</div>
					</div>
					<div>
						<strong>分页命名模板：</strong>
						<div class="ml-4 space-y-1">
							<div><code>{'{{title}}'}</code> → <span class="text-gray-600">庄心妍的街头采访（单P视频）</span></div>
							<div><code>{'{{ptitle}}'}</code> → <span class="text-gray-600">庄心妍的街头采访（使用分页标题）</span></div>
						</div>
					</div>
					<div>
						<strong>多P视频命名模板：</strong>
						<div class="ml-4 space-y-1">
							<div><code>{'{{title}}-P{{pid_pad}}'}</code> → <span class="text-gray-600">视频标题-P001.mp4（推荐格式）</span></div>
							<div><code>{'S{{season_pad}}E{{pid_pad}}-{{pid_pad}}'}</code> → <span class="text-gray-600">S01E01-01.mp4（番剧格式）</span></div>
							<div><code>{'{{ptitle}}'}</code> → <span class="text-gray-600">使用分页标题命名</span></div>
							<div><code>{'第{{pid}}集'}</code> → <span class="text-gray-600">第1集.mp4、第2集.mp4</span></div>
						</div>
					</div>
					<div>
						<strong>番剧命名模板：</strong>
						<div class="ml-4 space-y-1">
							<div><code>{'S{{season_pad}}E{{pid_pad}}-{{pid_pad}}'}</code> → <span class="text-gray-600">S01E01-01.mp4（番剧格式）</span></div>
						</div>
					</div>
					<div>
						<strong>文件夹结构模板：</strong>
						<div class="ml-4 space-y-1">
							<div><code>Season 1</code> → <span class="text-gray-600">多P视频的分季文件夹</span></div>
							<div><code>{'第{{pid}}季'}</code> → <span class="text-gray-600">第1季、第2季...</span></div>
						</div>
					</div>
					<div>
						<strong>时间格式示例：</strong>
						<div class="ml-4 space-y-1">
							<div><code>%Y-%m-%d</code> → <span class="text-gray-600">2023-12-25</span></div>
							<div><code>%Y年%m月%d日</code> → <span class="text-gray-600">2023年12月25日</span></div>
							<div><code>%Y-%m-%d %H:%M</code> → <span class="text-gray-600">2023-12-25 14:30</span></div>
						</div>
					</div>
					<div>
						<strong>截取函数示例：</strong>
						<div class="ml-4 space-y-1">
							<div><code>{'{{ truncate title 20 }}'}</code> → <span class="text-gray-600">截取标题前20个字符</span></div>
							<div><code>{'{{ truncate upper_name 10 }} - {{title}}'}</code> → <span class="text-gray-600">庄心妍 - 没想到吧～这些歌原来是我唱的！</span></div>
						</div>
					</div>
				</div>
			</div>
		</div>
	{/if}
	
	{#if loadingConfig}
		<div class="flex justify-center items-center py-8">
			<div class="text-gray-500">加载配置中...</div>
		</div>
	{:else}
		<form on:submit|preventDefault={handleSubmit} class="space-y-4">
			<div>
				<label class="block text-sm font-medium mb-1" for="video-name">
					视频文件夹命名模板
				</label>
				<Input 
					id="video-name" 
					bind:value={video_name} 
					placeholder={'例如：{{title}}'}
					class="bg-gray-50 text-gray-900 border-gray-300 focus:border-blue-500 focus:ring-1 focus:ring-blue-500"
				/>
				<p class="text-xs text-gray-500 mt-1">{fieldDescriptions.video_name}</p>
			</div>
			
			<div>
				<label class="block text-sm font-medium mb-1" for="page-name">
					视频分页命名模板
				</label>
				<Input 
					id="page-name" 
					bind:value={page_name} 
					placeholder={'例如：{{title}}'}
					class="bg-gray-50 text-gray-900 border-gray-300 focus:border-blue-500 focus:ring-1 focus:ring-blue-500"
				/>
				<p class="text-xs text-gray-500 mt-1">{fieldDescriptions.page_name}</p>
			</div>
			
			<div>
				<label class="block text-sm font-medium mb-1" for="multi-page-name">
					多P视频分页命名模板
				</label>
				<Input 
					id="multi-page-name" 
					bind:value={multi_page_name} 
					placeholder={'例如：{{title}}-P{{pid_pad}}'}
					class="bg-gray-50 text-gray-900 border-gray-300 focus:border-blue-500 focus:ring-1 focus:ring-blue-500"
				/>
				<p class="text-xs text-gray-500 mt-1">{fieldDescriptions.multi_page_name}</p>
				<div class="mt-2 p-2 bg-red-50 border border-red-200 rounded-md">
					<p class="text-xs text-red-600 font-medium">⚠️ 重要提醒：</p>
					<p class="text-xs text-red-600">模板必须包含分页标识符（如 {'{{pid}}'} 或 {'{{pid_pad}}'}），否则所有分页文件会重名并相互覆盖！</p>
				</div>
			</div>
			
			<div>
				<label class="block text-sm font-medium mb-1" for="bangumi-name">
					番剧命名模板
				</label>
				<Input 
					id="bangumi-name" 
					bind:value={bangumi_name} 
					placeholder={'例如：S{{season_pad}}E{{pid_pad}}-{{pid_pad}}'}
					class="bg-gray-50 text-gray-900 border-gray-300 focus:border-blue-500 focus:ring-1 focus:ring-blue-500"
				/>
				<p class="text-xs text-gray-500 mt-1">{fieldDescriptions.bangumi_name}</p>
				<div class="mt-2 p-2 bg-red-50 border border-red-200 rounded-md">
					<p class="text-xs text-red-600 font-medium">⚠️ 重要提醒：</p>
					<p class="text-xs text-red-600">模板必须包含分页标识符（如 {'{{pid}}'} 或 {'{{pid_pad}}'}），否则所有分页文件会重名并相互覆盖！</p>
				</div>
			</div>
			
			<div>
				<label class="block text-sm font-medium mb-1" for="folder-structure">
					多页视频文件夹结构模板
				</label>
				<Input 
					id="folder-structure" 
					bind:value={folder_structure} 
					placeholder="例如：Season 1"
					class="bg-gray-50 text-gray-900 border-gray-300 focus:border-blue-500 focus:ring-1 focus:ring-blue-500"
				/>
				<p class="text-xs text-gray-500 mt-1">{fieldDescriptions.folder_structure}</p>
			</div>
			
			<div>
				<label class="block text-sm font-medium mb-1" for="time-format">
					时间格式化模板
				</label>
				<Input 
					id="time-format" 
					bind:value={time_format} 
					placeholder="例如：%Y-%m-%d"
					class="bg-gray-50 text-gray-900 border-gray-300 focus:border-blue-500 focus:ring-1 focus:ring-blue-500"
				/>
				<p class="text-xs text-gray-500 mt-1">{fieldDescriptions.time_format}</p>
			</div>
			
			<div>
				<label class="block text-sm font-medium mb-1" for="interval">
					自动扫描间隔时间（秒）
				</label>
				<Input 
					id="interval" 
					type="number" 
					bind:value={interval} 
					min="60"
					placeholder="例如：1200"
					class="bg-gray-50 text-gray-900 border-gray-300 focus:border-blue-500 focus:ring-1 focus:ring-blue-500"
				/>
				<p class="text-xs text-gray-500 mt-1">{fieldDescriptions.interval}</p>
			</div>
			
			<div>
				<label class="block text-sm font-medium mb-1" for="nfo-time-type">
					NFO文件时间类型选择
				</label>
				<select 
					id="nfo-time-type" 
					class="w-full p-2 border rounded bg-gray-50 text-gray-900 border-gray-300 focus:border-blue-500 focus:ring-1 focus:ring-blue-500" 
					bind:value={nfo_time_type}
				>
					{#each nfoTimeTypeOptions as option}
						<option value={option.value}>{option.label}</option>
					{/each}
				</select>
				<p class="text-xs text-gray-500 mt-1">{fieldDescriptions.nfo_time_type}</p>
			</div>
			
			<div class="flex justify-end space-x-2">
				<Button type="button" variant="outline" on:click={loadConfig} disabled={loading || loadingConfig}>
					重置
				</Button>
				<Button type="submit" disabled={loading || loadingConfig}>
					{loading ? '更新中...' : '保存配置'}
				</Button>
			</div>
		</form>
	{/if}
</div> 