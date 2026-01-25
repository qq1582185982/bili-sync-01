import type { SortBy, SortOrder, VideosRequest } from '$lib/types';

export type VideoSourceFilter = { type: string; id: string } | null | undefined;

export function buildVideosRequest({
	page,
	pageSize,
	query,
	videoSource,
	showFailedOnly,
	minHeight,
	maxHeight,
	sortBy = 'id',
	sortOrder = 'desc'
}: {
	page: number;
	pageSize: number;
	query?: string;
	videoSource?: VideoSourceFilter;
	showFailedOnly?: boolean;
	minHeight?: number | null;
	maxHeight?: number | null;
	sortBy?: SortBy;
	sortOrder?: SortOrder;
}): VideosRequest {
	const params: VideosRequest = {
		page,
		page_size: pageSize,
		sort_by: sortBy,
		sort_order: sortOrder
	};

	if (query?.trim()) {
		params.query = query;
	}

	if (showFailedOnly) {
		params.show_failed_only = true;
	}

	if (typeof minHeight === 'number' && Number.isFinite(minHeight)) {
		params.min_height = minHeight;
	}
	if (typeof maxHeight === 'number' && Number.isFinite(maxHeight)) {
		params.max_height = maxHeight;
	}

	if (videoSource?.type && videoSource.id) {
		const sourceId = Number.parseInt(videoSource.id, 10);
		if (Number.isFinite(sourceId)) {
			switch (videoSource.type) {
				case 'collection':
					params.collection = sourceId;
					break;
				case 'favorite':
					params.favorite = sourceId;
					break;
				case 'submission':
					params.submission = sourceId;
					break;
				case 'watch_later':
					params.watch_later = sourceId;
					break;
				case 'bangumi':
					params.bangumi = sourceId;
					break;
			}
		}
	}

	return params;
}
