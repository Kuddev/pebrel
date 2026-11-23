//! 保留协议列表已经返回的大小，避免为画时间线额外下载归档或逐个发 HEAD。
use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Snapshot {
    pub name: String,
    pub bytes: Option<u64>,
}

/// 列表只提取固定元素的文本；归档名称仍经过父模块的 ASCII 白名单校验。
fn elements<'a>(xml: &'a str, wanted: &str) -> Vec<&'a str> {
    let mut values = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find('<') {
        rest = &rest[start + 1..];
        let Some(end) = rest.find('>') else { break };
        let tag = rest[..end].split_whitespace().next().unwrap_or_default();
        if !tag.starts_with('/') && tag.rsplit(':').next() == Some(wanted) {
            let close = format!("</{tag}>");
            let body = &rest[end + 1..];
            if let Some(end) = body.find(&close) {
                values.push(body[..end].trim());
                rest = &body[end + close.len()..];
                continue;
            }
        }
        rest = &rest[end + 1..];
    }
    values
}

pub(super) fn webdav(xml: &str) -> Vec<Snapshot> {
    elements(xml, "response")
        .into_iter()
        .filter_map(|record| {
            let href = *elements(record, "href").first()?;
            let name = href.trim_end_matches('/').rsplit('/').next()?;
            is_archive_name(name).then(|| Snapshot {
                name: name.to_owned(),
                bytes: elements(record, "getcontentlength")
                    .first()
                    .and_then(|value| value.parse().ok()),
            })
        })
        .collect()
}

pub(super) fn s3(xml: &str) -> Vec<Snapshot> {
    elements(xml, "Contents")
        .into_iter()
        .filter_map(|record| {
            let key = *elements(record, "Key").first()?;
            let name = key.rsplit('/').next()?;
            is_archive_name(name).then(|| Snapshot {
                name: name.to_owned(),
                bytes: elements(record, "Size").first().and_then(|value| value.parse().ok()),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_stay_with_their_record_and_unknown_sizes_are_not_zero() {
        let entries = webdav(
            r#"<d:multistatus><d:response><d:href>/a/pebrel-backup-20260927-010203.nbk</d:href><d:propstat><d:prop><d:getcontentlength>42</d:getcontentlength></d:prop></d:propstat></d:response><D:response><D:href>/a/pebrel-backup-20260926-010203.nbk</D:href></D:response></d:multistatus>"#,
        );
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].bytes, Some(42));
        assert_eq!(entries[1].bytes, None);
        let entries = s3(
            "<ListBucketResult><Contents><Key>prefix/pebrel-backup-20260927-010203.nbk</Key><Size>96</Size></Contents><Contents><Key>keep.txt</Key><Size>20</Size></Contents></ListBucketResult>",
        );
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].bytes, Some(96));
    }
}
