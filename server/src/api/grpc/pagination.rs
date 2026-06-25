use super::convert::saturating_u32;
use crate::pb;

fn limit_from_u32(limit: u32) -> usize {
    usize::try_from(limit).unwrap_or(usize::MAX)
}

pub(super) fn request_limit(legacy_limit: u32, page: Option<&pb::PageRequest>) -> usize {
    let page_size = page
        .and_then(|page| (page.page_size > 0).then_some(page.page_size))
        .unwrap_or(legacy_limit);

    limit_from_u32(page_size)
}

pub(super) fn truncate_to_limit<T>(items: &mut Vec<T>, limit: usize) {
    if limit > 0 {
        items.truncate(limit);
    }
}

pub(super) fn page_info(returned_count: usize) -> pb::PageInfo {
    pb::PageInfo {
        next_page_token: String::new(),
        returned_count: saturating_u32(returned_count),
    }
}
