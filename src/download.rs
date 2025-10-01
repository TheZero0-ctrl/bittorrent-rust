use futures_util::task::waker;

use crate::torrent::File;

pub async fn all() -> anyhow::Result<Downloaded> {
}

pub struct Downloaded {
    bytes: Vec<u8>,
    files: Vec<File>
}

impl<'a> IntoIterator for &'a Downloaded {
    type Item = &'a DownloadedFile;
    type IntoIter = DownloadedIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        DownloadedIter::new(self)
    }
}

pub struct DownloadedIter<'d> {
    downloaded: &'d Downloaded,
    file_iter: std::slice::Iter<'d, File>,
    offset: usize
}

impl<'d> DownloadedIter<'d> {
    fn new(downloaded: &'d Downloaded) -> Self {
        Self {
            downloaded,
            file_iter: downloaded.files.iter(),
            offset: 0
        }
    }
}

impl<'d> Iterator for DownloadedIter<'d> {
    type Item = &'d File;
    fn next(&mut self) -> Option<Self::Item> {
        let file = self.file_iter.next()?;
        let bytes = &se
    }
}
