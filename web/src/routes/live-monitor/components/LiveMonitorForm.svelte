<script lang="ts">
	import { createEventDispatcher, onMount } from 'svelte';
	// 使用现有的基础组件
	import * as Button from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import type { LiveMonitorConfig, LiveFormat, QualityInfo } from '$lib/types';
	import api from '$lib/api';

	export let monitor: LiveMonitorConfig | null = null;
	export let onSubmit: (data: any) => void;
	export let onCancel: () => void;

	const dispatch = createEventDispatcher();

	// 表单数据
	let formData = {
		upper_id: monitor?.upper_id || 0,
		upper_name: monitor?.upper_name || '',
		room_id: monitor?.room_id || 0,
		short_room_id: monitor?.short_room_id || undefined,
		path: monitor?.path || '',
		enabled: monitor?.enabled ?? true,
		check_interval: monitor?.check_interval || 60,
		quality_level: monitor?.quality_level || 10000, // 默认原画
		format: monitor?.format || 'flv' as LiveFormat
	};

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

	// 格式选项
	const formatOptions = [
		{ value: 'flv', label: 'FLV' },
		{ value: 'mp4', label: 'MP4' }
	];

	// 表单验证
	let errors: Record<string, string> = {};

	function validateForm() {
		errors = {};

		if (!formData.upper_id || formData.upper_id <= 0) {
			errors.upper_id = 'UP主ID必须大于0';
		}

		if (!formData.upper_name.trim()) {
			errors.upper_name = 'UP主名称不能为空';
		}

		if (!formData.room_id || formData.room_id <= 0) {
			errors.room_id = '直播间ID必须大于0';
		}

		if (!formData.path.trim()) {
			errors.path = '保存路径不能为空';
		}

		if (formData.check_interval < 10 || formData.check_interval > 3600) {
			errors.check_interval = '检查间隔必须在10-3600秒之间';
		}

		return Object.keys(errors).length === 0;
	}

	function handleSubmit() {
		if (!validateForm()) return;

		const submitData = {
			...formData,
			short_room_id: formData.short_room_id || undefined
		};

		onSubmit(submitData);
	}

	// 组件挂载时加载质量等级选项
	onMount(() => {
		loadQualityOptions();
	});
</script>

<div class="space-y-6">
	<div class="grid gap-4">
		<!-- UP主信息 -->
		<div class="grid grid-cols-2 gap-4">
			<div class="space-y-2">
				<Label for="upper_id">UP主ID *</Label>
				<Input
					id="upper_id"
					type="number"
					bind:value={formData.upper_id}
					placeholder="请输入UP主ID"
					class={errors.upper_id ? 'border-destructive' : ''}
				/>
				{#if errors.upper_id}
					<p class="text-sm text-destructive">{errors.upper_id}</p>
				{/if}
			</div>

			<div class="space-y-2">
				<Label for="upper_name">UP主名称 *</Label>
				<Input
					id="upper_name"
					bind:value={formData.upper_name}
					placeholder="请输入UP主名称"
					class={errors.upper_name ? 'border-destructive' : ''}
				/>
				{#if errors.upper_name}
					<p class="text-sm text-destructive">{errors.upper_name}</p>
				{/if}
			</div>
		</div>

		<!-- 直播间信息 -->
		<div class="grid grid-cols-2 gap-4">
			<div class="space-y-2">
				<Label for="room_id">直播间ID *</Label>
				<Input
					id="room_id"
					type="number"
					bind:value={formData.room_id}
					placeholder="请输入直播间ID"
					class={errors.room_id ? 'border-destructive' : ''}
				/>
				{#if errors.room_id}
					<p class="text-sm text-destructive">{errors.room_id}</p>
				{/if}
			</div>

			<div class="space-y-2">
				<Label for="short_room_id">短直播间ID</Label>
				<Input
					id="short_room_id"
					type="number"
					bind:value={formData.short_room_id}
					placeholder="可选，短直播间ID"
				/>
			</div>
		</div>

		<!-- 保存路径 -->
		<div class="space-y-2">
			<Label for="path">保存路径 *</Label>
			<Input
				id="path"
				bind:value={formData.path}
				placeholder="例如：/Downloads/直播录制/UP主名称"
				class={errors.path ? 'border-destructive' : ''}
			/>
			{#if errors.path}
				<p class="text-sm text-destructive">{errors.path}</p>
			{/if}
			<p class="text-sm text-muted-foreground">
				录制文件将保存到此路径下，支持相对路径和绝对路径
			</p>
		</div>

		<!-- 监控设置 -->
		<div class="grid grid-cols-3 gap-4">
			<div class="space-y-2">
				<Label for="check_interval">检查间隔（秒）*</Label>
				<Input
					id="check_interval"
					type="number"
					min="10"
					max="3600"
					bind:value={formData.check_interval}
					class={errors.check_interval ? 'border-destructive' : ''}
				/>
				{#if errors.check_interval}
					<p class="text-sm text-destructive">{errors.check_interval}</p>
				{/if}
			</div>

			<div class="space-y-2">
				<Label for="quality_level">录制画质</Label>
				<select
					id="quality_level"
					bind:value={formData.quality_level}
					class="flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm ring-offset-background placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50"
				>
					{#each qualityOptions as option}
						<option value={option.qn}>{option.name}</option>
					{/each}
				</select>
				<p class="text-xs text-muted-foreground">
					{#if formData.quality_level}
						{qualityOptions.find(q => q.qn === formData.quality_level)?.description || 'B站质量等级'}
					{:else}
						选择录制质量等级，数值越高画质越好
					{/if}
				</p>
			</div>

			<div class="space-y-2">
				<Label for="format">录制格式</Label>
				<select
					id="format"
					bind:value={formData.format}
					class="flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm ring-offset-background placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50"
				>
					{#each formatOptions as option}
						<option value={option.value}>{option.label}</option>
					{/each}
				</select>
			</div>
		</div>

		<!-- 启用状态 -->
		<div class="flex items-center space-x-2">
			<input 
				type="checkbox" 
				bind:checked={formData.enabled} 
				id="enabled"
				class="h-4 w-4 rounded border-gray-300 text-blue-600 focus:ring-blue-500"
			/>
			<Label for="enabled">启用监控</Label>
		</div>
	</div>

	<!-- 提示信息 -->
	<div class="rounded-md bg-muted p-4 text-sm">
		<h4 class="font-medium mb-2">使用说明：</h4>
		<ul class="space-y-1 text-muted-foreground">
			<li>• UP主ID可以从UP主主页URL获取：https://space.bilibili.com/UP主ID</li>
			<li>• 直播间ID可以从直播间URL获取：https://live.bilibili.com/直播间ID</li>
			<li>• 检查间隔建议设置为30-60秒，太频繁可能被限制</li>
			<li>• 原画画质（10000）需要大量存储空间，数值越高画质越好</li>
		</ul>
	</div>

	<!-- 操作按钮 -->
	<div class="flex justify-end space-x-2">
		<Button.Root variant="outline" on:click={onCancel}>
			取消
		</Button.Root>
		<Button.Root on:click={handleSubmit}>
			{monitor ? '更新' : '创建'}
		</Button.Root>
	</div>
</div>