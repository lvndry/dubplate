//! The DeviceSQL container shared by `export.pdb` and `exportExt.pdb`: the file header, the
//! table directory, index pages, and data pages whose rows sit in a heap above a directory of
//! row offsets.

use std::collections::HashSet;

use super::error::DeviceLibraryError;

/// The page size of every export rekordbox writes.
pub(crate) const PAGE_SIZE: usize = 4096;
const PAGE_HEADER_LENGTH: usize = 0x28;
const ROW_GROUP_LENGTH: usize = 0x24;
const ROWS_PER_GROUP: usize = 16;
const FILE_HEADER_LENGTH: usize = 0x1c;
const TABLE_ENTRY_LENGTH: usize = 16;
/// Header field 0x10, which rekordbox sets to 5 in the exports players read.
const FILE_HEADER_UNKNOWN: u32 = 5;
const DATA_PAGE_FLAGS: u8 = 0x24;
const INDEX_PAGE_FLAG: u8 = 0x40;
const INDEX_PAGE_FLAGS: u8 = 0x64;
/// The "no transaction" marker of an index page's header.
const NO_TRANSACTION: u16 = 0x1fff;
/// Header field 0x24 of every index page.
const INDEX_PAGE_MARKER: u16 = 1004;
/// The page number an index page holds where it has no data page to name.
const NO_PAGE: u32 = 0x03ff_ffff;
/// The filler of an index page's empty entries.
const INDEX_FILLER: u32 = 0x1fff_fff8;
const INDEX_FILLER_COUNT: usize = 1004;
const SLOT_COUNT_BITS: u32 = 13;
const SLOT_COUNT_MASK: u32 = (1 << SLOT_COUNT_BITS) - 1;
/// Where a row keeps its place in its page, as `0x20` times its slot, for the row kinds that
/// have one.
const INDEX_SHIFT_POSITION: usize = 2;
const INDEX_SHIFT_STEP: usize = 0x20;

/// A page's number in its file: its byte offset divided by the page size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PageIndex(pub u32);

impl PageIndex {
    fn byte_offset(self, page_size: usize) -> usize {
        self.0 as usize * page_size
    }
}

impl std::fmt::Display for PageIndex {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

/// Where a row starts in its page's heap, counted from the end of the page header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct HeapOffset(u16);

impl HeapOffset {
    fn page_position(self) -> usize {
        PAGE_HEADER_LENGTH + usize::from(self.0)
    }
}

/// A row ready to be placed in a page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EncodedRow {
    /// The row's bytes.
    pub bytes: Vec<u8>,
    /// The heap space rekordbox reserves for the row, at least its length and a multiple of 4.
    pub allocation: usize,
    /// Whether bytes 2 and 3 hold the row's place in its page.
    pub has_index_shift: bool,
}

impl EncodedRow {
    /// A row with no place field, given the space rekordbox reserves for it.
    pub fn new(bytes: Vec<u8>, allocation: usize) -> Self {
        Self {
            allocation: align4(allocation.max(bytes.len())),
            bytes,
            has_index_shift: false,
        }
    }

    /// A row whose bytes 2 and 3 hold its place in its page.
    pub fn with_index_shift(bytes: Vec<u8>, allocation: usize) -> Self {
        Self {
            has_index_shift: true,
            ..Self::new(bytes, allocation)
        }
    }
}

/// The rows of one table, by the table's type number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TableRows {
    pub type_number: u32,
    pub rows: Vec<EncodedRow>,
}

/// `value` rounded up to a multiple of 4.
pub(crate) const fn align4(value: usize) -> usize {
    (value + 3) & !3
}

/// Writes a database with `tables` in this order, as rekordbox lays out a fresh export: page 0
/// holds the header, table `t` has its index page at `2t + 1` and its first data page (or, when
/// empty, the page waiting for its first row) at `2t + 2`, and further pages follow the last of
/// those.
pub(crate) fn write_database(tables: &[TableRows]) -> Result<Vec<u8>, DeviceLibraryError> {
    let table_count = u32::try_from(tables.len()).map_err(|_| DeviceLibraryError::TooManyPages)?;
    let mut next_free = 2 * table_count + 1;
    let mut allocate = || {
        let page = PageIndex(next_free);
        next_free = next_free
            .checked_add(1)
            .ok_or(DeviceLibraryError::TooManyPages)?;
        Ok::<PageIndex, DeviceLibraryError>(page)
    };

    let mut layouts = Vec::with_capacity(tables.len());
    for (position, table) in (0..table_count).zip(tables) {
        let pages = place_rows(table)?;
        let mut data_pages = Vec::with_capacity(pages.len());
        for index in 0..pages.len() {
            data_pages.push(if index == 0 {
                PageIndex(2 * position + 2)
            } else {
                allocate()?
            });
        }
        layouts.push(TableLayout {
            index_page: PageIndex(2 * position + 1),
            data_pages,
            pages,
            empty_candidate: PageIndex(0),
        });
    }
    for (position, layout) in (0..table_count).zip(&mut layouts) {
        layout.empty_candidate = if layout.data_pages.is_empty() {
            PageIndex(2 * position + 2)
        } else {
            allocate()?
        };
    }
    let next_unused_page = next_free;

    let last_written = layouts
        .iter()
        .flat_map(|layout| layout.data_pages.iter().map(|page| page.0))
        .chain([(2 * table_count).saturating_sub(1)])
        .max()
        .unwrap_or(0);
    let page_count =
        usize::try_from(last_written).map_err(|_| DeviceLibraryError::TooManyPages)? + 1;
    let mut file = vec![0_u8; page_count * PAGE_SIZE];

    let mut sequence = 1;
    for (table, layout) in tables.iter().zip(&layouts) {
        let first_data = layout.data_pages.first().copied();
        let index_next = first_data.unwrap_or(layout.empty_candidate);
        write_index_page(
            page_mut(&mut file, layout.index_page),
            layout.index_page,
            table.type_number,
            first_data,
            index_next,
        );
        if layout.data_pages.is_empty() {
            continue;
        }
        sequence += 1;
        for (place, (page_index, rows)) in layout.data_pages.iter().zip(&layout.pages).enumerate() {
            let next = layout
                .data_pages
                .get(place + 1)
                .copied()
                .unwrap_or(layout.empty_candidate);
            let page = DataPage {
                index: *page_index,
                type_number: table.type_number,
                next,
                sequence,
                rows,
            };
            page.write(page_mut(&mut file, *page_index));
        }
    }

    let header = &mut file[..FILE_HEADER_LENGTH + tables.len() * TABLE_ENTRY_LENGTH];
    put_u32(header, 0x04, PAGE_SIZE as u32);
    put_u32(header, 0x08, table_count);
    put_u32(header, 0x0c, next_unused_page);
    put_u32(header, 0x10, FILE_HEADER_UNKNOWN);
    put_u32(header, 0x14, sequence + 1);
    for (place, (table, layout)) in tables.iter().zip(&layouts).enumerate() {
        let entry = FILE_HEADER_LENGTH + place * TABLE_ENTRY_LENGTH;
        put_u32(header, entry, table.type_number);
        put_u32(header, entry + 4, layout.empty_candidate.0);
        put_u32(header, entry + 8, layout.index_page.0);
        put_u32(
            header,
            entry + 12,
            layout
                .data_pages
                .last()
                .copied()
                .unwrap_or(layout.index_page)
                .0,
        );
    }
    Ok(file)
}

struct TableLayout<'rows> {
    index_page: PageIndex,
    data_pages: Vec<PageIndex>,
    pages: Vec<Vec<&'rows EncodedRow>>,
    empty_candidate: PageIndex,
}

fn page_mut(file: &mut [u8], page: PageIndex) -> &mut [u8] {
    let start = page.byte_offset(PAGE_SIZE);
    &mut file[start..start + PAGE_SIZE]
}

/// Fills pages in order, starting a new one when the next row and its directory slot no longer
/// fit beside the rows already there.
fn place_rows(table: &TableRows) -> Result<Vec<Vec<&EncodedRow>>, DeviceLibraryError> {
    let mut pages: Vec<Vec<&EncodedRow>> = Vec::new();
    let mut current: Vec<&EncodedRow> = Vec::new();
    let mut heap_used = 0;
    for row in &table.rows {
        if !fits(row.allocation, 1) {
            return Err(DeviceLibraryError::RowTooLarge {
                table: super::TableKind::from_type_number(table.type_number)
                    .unwrap_or(super::TableKind::Unknown9),
                length: row.bytes.len(),
            });
        }
        if !current.is_empty() && !fits(heap_used + row.allocation, current.len() + 1) {
            pages.push(std::mem::take(&mut current));
            heap_used = 0;
        }
        heap_used += row.allocation;
        current.push(row);
    }
    if !current.is_empty() {
        pages.push(current);
    }
    Ok(pages)
}

fn fits(heap_length: usize, slots: usize) -> bool {
    let directory = slots.div_ceil(ROWS_PER_GROUP) * ROW_GROUP_LENGTH;
    PAGE_HEADER_LENGTH + heap_length + directory <= PAGE_SIZE && slots <= SLOT_COUNT_MASK as usize
}

struct DataPage<'rows> {
    index: PageIndex,
    type_number: u32,
    next: PageIndex,
    sequence: u32,
    rows: &'rows [&'rows EncodedRow],
}

impl DataPage<'_> {
    fn write(&self, page: &mut [u8]) {
        let slot_count = self.rows.len();
        let mut heap_offset = 0;
        for (slot, row) in self.rows.iter().enumerate() {
            let offset = HeapOffset(heap_offset as u16);
            let start = offset.page_position();
            page[start..start + row.bytes.len()].copy_from_slice(&row.bytes);
            if row.has_index_shift {
                let shift = (slot * INDEX_SHIFT_STEP) as u16;
                put_u16(page, start + INDEX_SHIFT_POSITION, shift);
            }
            let group_base = PAGE_SIZE - (slot / ROWS_PER_GROUP) * ROW_GROUP_LENGTH;
            put_u16(page, group_base - 6 - 2 * (slot % ROWS_PER_GROUP), offset.0);
            heap_offset += row.allocation;
        }
        for group in 0..slot_count.div_ceil(ROWS_PER_GROUP) {
            let rows_in_group = (slot_count - group * ROWS_PER_GROUP).min(ROWS_PER_GROUP);
            let mask = if rows_in_group == ROWS_PER_GROUP {
                u16::MAX
            } else {
                (1_u16 << rows_in_group) - 1
            };
            let group_base = PAGE_SIZE - group * ROW_GROUP_LENGTH;
            put_u16(page, group_base - 4, mask);
            put_u16(page, group_base - 2, mask);
        }

        let slots = slot_count as u32;
        let counts = slots | (slots << SLOT_COUNT_BITS);
        let directory_used = 2 * slot_count + 4 * slot_count.div_ceil(ROWS_PER_GROUP);
        let free = PAGE_SIZE - PAGE_HEADER_LENGTH - heap_offset - directory_used;
        put_u32(page, 0x04, self.index.0);
        put_u32(page, 0x08, self.type_number);
        put_u32(page, 0x0c, self.next.0);
        put_u32(page, 0x10, self.sequence);
        page[0x18..0x1b].copy_from_slice(&counts.to_le_bytes()[..3]);
        page[0x1b] = DATA_PAGE_FLAGS;
        put_u16(page, 0x1c, free as u16);
        put_u16(page, 0x1e, heap_offset as u16);
        put_u16(page, 0x20, slot_count as u16);
    }
}

fn write_index_page(
    page: &mut [u8],
    index: PageIndex,
    type_number: u32,
    first_data: Option<PageIndex>,
    next: PageIndex,
) {
    put_u32(page, 0x04, index.0);
    put_u32(page, 0x08, type_number);
    put_u32(page, 0x0c, next.0);
    put_u32(page, 0x10, 1);
    page[0x1b] = INDEX_PAGE_FLAGS;
    put_u16(page, 0x20, NO_TRANSACTION);
    put_u16(page, 0x22, NO_TRANSACTION);
    put_u16(page, 0x24, INDEX_PAGE_MARKER);
    let heap = PAGE_HEADER_LENGTH;
    put_u32(page, heap, index.0);
    put_u32(page, heap + 4, first_data.map_or(NO_PAGE, |page| page.0));
    put_u32(page, heap + 8, NO_PAGE);
    put_u16(page, heap + 0x12, NO_TRANSACTION);
    for entry in 0..INDEX_FILLER_COUNT {
        put_u32(page, heap + 0x14 + 4 * entry, INDEX_FILLER);
    }
}

fn put_u16(bytes: &mut [u8], position: usize, value: u16) {
    bytes[position..position + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(bytes: &mut [u8], position: usize, value: u32) {
    bytes[position..position + 4].copy_from_slice(&value.to_le_bytes());
}

pub(crate) fn get_u16(bytes: &[u8], position: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(position..position + 2)?.try_into().ok()?,
    ))
}

pub(crate) fn get_u32(bytes: &[u8], position: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(position..position + 4)?.try_into().ok()?,
    ))
}

/// Whether a page holds rows or is a table's index page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageType {
    /// An index page, which every table starts with and which holds no rows.
    Index,
    /// A page of rows.
    Data,
}

/// A page's header, as read from the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageSummary {
    /// The page's number.
    pub index: PageIndex,
    /// The table type number the page says it belongs to.
    pub type_number: u32,
    /// The page that follows it in its table, or the table's empty candidate after the last.
    pub next_page: PageIndex,
    /// The write generation stamped into the page.
    pub sequence: u32,
    /// Index page or data page.
    pub page_type: PageType,
    /// The raw flags byte.
    pub flags: u8,
    /// Row slots ever allocated in the page, deleted ones included.
    pub slot_count: u16,
    /// Rows the page says are present.
    pub present_count: u16,
    /// Free heap bytes, as the page says.
    pub free_size: u16,
    /// Used heap bytes, as the page says.
    pub used_size: u16,
}

/// One table of a database file, as read: its directory entry, its pages and its present rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableSummary {
    /// The table type number.
    pub type_number: u32,
    /// The page waiting for the table's next rows.
    pub empty_candidate: PageIndex,
    /// The table's first page, its index page in every export rekordbox writes.
    pub first_page: PageIndex,
    /// The table's last page.
    pub last_page: PageIndex,
    /// The pages, in chain order.
    pub pages: Vec<PageSummary>,
    /// Every present row's bytes, from the row's start to the start of the next row, in page
    /// and slot order.
    pub rows: Vec<Vec<u8>>,
}

/// A DeviceSQL database read from its bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseFile {
    /// The page size.
    pub page_size: u32,
    /// The first page no table uses yet.
    pub next_unused_page: PageIndex,
    /// Header field 0x10.
    pub header_unknown: u32,
    /// The database's write sequence.
    pub sequence: u32,
    /// The tables, in directory order.
    pub tables: Vec<TableSummary>,
}

impl DatabaseFile {
    /// Reads the database in `bytes`: the header, every table's page chain and every present
    /// row. Pages past the end of the file, loops in a chain and rows outside their page are
    /// errors.
    pub fn parse(bytes: &[u8]) -> Result<Self, DeviceLibraryError> {
        let header_field = |position: usize| {
            get_u32(bytes, position).ok_or_else(|| {
                DeviceLibraryError::NotADatabase("the file is shorter than its header".to_owned())
            })
        };
        if header_field(0)? != 0 {
            return Err(DeviceLibraryError::NotADatabase(
                "it does not start with four zero bytes".to_owned(),
            ));
        }
        let page_size = header_field(0x04)?;
        if !(512..=65_536).contains(&page_size) || !page_size.is_power_of_two() {
            return Err(DeviceLibraryError::NotADatabase(format!(
                "its page size {page_size} is not a page size"
            )));
        }
        let table_count = header_field(0x08)?;
        let mut tables = Vec::new();
        for place in 0..table_count as usize {
            let entry = FILE_HEADER_LENGTH + place * TABLE_ENTRY_LENGTH;
            let type_number = header_field(entry)?;
            let empty_candidate = PageIndex(header_field(entry + 4)?);
            let first_page = PageIndex(header_field(entry + 8)?);
            let last_page = PageIndex(header_field(entry + 12)?);
            let (pages, rows) = read_chain(bytes, page_size as usize, first_page, last_page)?;
            tables.push(TableSummary {
                type_number,
                empty_candidate,
                first_page,
                last_page,
                pages,
                rows,
            });
        }
        Ok(Self {
            page_size,
            next_unused_page: PageIndex(header_field(0x0c)?),
            header_unknown: header_field(0x10)?,
            sequence: header_field(0x14)?,
            tables,
        })
    }

    /// The table with type number `type_number`.
    pub fn table(&self, type_number: u32) -> Option<&TableSummary> {
        self.tables
            .iter()
            .find(|table| table.type_number == type_number)
    }
}

fn read_chain(
    bytes: &[u8],
    page_size: usize,
    first_page: PageIndex,
    last_page: PageIndex,
) -> Result<(Vec<PageSummary>, Vec<Vec<u8>>), DeviceLibraryError> {
    let mut pages = Vec::new();
    let mut rows = Vec::new();
    let mut visited = HashSet::new();
    let mut current = first_page;
    loop {
        if !visited.insert(current) {
            return Err(DeviceLibraryError::MalformedPage {
                page: current,
                reason: "the table's pages run in a loop".to_owned(),
            });
        }
        let start = current.byte_offset(page_size);
        let page = bytes.get(start..start + page_size).ok_or_else(|| {
            DeviceLibraryError::MalformedPage {
                page: current,
                reason: "it lies past the end of the file".to_owned(),
            }
        })?;
        let summary = summarize(page, current);
        if summary.page_type == PageType::Data {
            read_rows(page, current, &summary, &mut rows)?;
        }
        let next = summary.next_page;
        pages.push(summary);
        if current == last_page {
            return Ok((pages, rows));
        }
        current = next;
    }
}

fn summarize(page: &[u8], index: PageIndex) -> PageSummary {
    let field16 = |position| get_u16(page, position).unwrap_or(0);
    let field32 = |position| get_u32(page, position).unwrap_or(0);
    let counts = u32::from_le_bytes([page[0x18], page[0x19], page[0x1a], 0]);
    let flags = page[0x1b];
    PageSummary {
        index,
        type_number: field32(0x08),
        next_page: PageIndex(field32(0x0c)),
        sequence: field32(0x10),
        page_type: if flags & INDEX_PAGE_FLAG == 0 {
            PageType::Data
        } else {
            PageType::Index
        },
        flags,
        slot_count: (counts & SLOT_COUNT_MASK) as u16,
        present_count: (counts >> SLOT_COUNT_BITS) as u16,
        free_size: field16(0x1c),
        used_size: field16(0x1e),
    }
}

fn read_rows(
    page: &[u8],
    index: PageIndex,
    summary: &PageSummary,
    rows: &mut Vec<Vec<u8>>,
) -> Result<(), DeviceLibraryError> {
    let malformed = |reason: String| DeviceLibraryError::MalformedPage {
        page: index,
        reason,
    };
    let page_size = page.len();
    let slot_count = usize::from(summary.slot_count);
    let groups = slot_count.div_ceil(ROWS_PER_GROUP);
    if PAGE_HEADER_LENGTH + groups * ROW_GROUP_LENGTH > page_size {
        return Err(malformed(format!(
            "its {slot_count} row slots need more room than the page has"
        )));
    }
    let heap_end = PAGE_HEADER_LENGTH + usize::from(summary.used_size);
    let mut slots = Vec::with_capacity(slot_count);
    for slot in 0..slot_count {
        let group_base = page_size - (slot / ROWS_PER_GROUP) * ROW_GROUP_LENGTH;
        let present_mask = get_u16(page, group_base - 4).unwrap_or(0);
        let offset =
            HeapOffset(get_u16(page, group_base - 6 - 2 * (slot % ROWS_PER_GROUP)).unwrap_or(0));
        let present = present_mask >> (slot % ROWS_PER_GROUP) & 1 == 1;
        slots.push((offset.page_position(), present));
    }
    for &(start, present) in &slots {
        if !present {
            continue;
        }
        let end = slots
            .iter()
            .map(|&(other, _)| other)
            .filter(|&other| other > start)
            .min()
            .unwrap_or(heap_end)
            .min(page_size);
        let row = page
            .get(start..end)
            .filter(|row| !row.is_empty())
            .ok_or_else(|| malformed(format!("a row at byte {start:#x} lies outside the heap")))?;
        rows.push(row.to_vec());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(type_number: u32, rows: Vec<EncodedRow>) -> TableRows {
        TableRows { type_number, rows }
    }

    #[test]
    fn tables_get_index_pages_and_data_follows_on_the_next_page() {
        let rows = (0..3_u8)
            .map(|value| EncodedRow::new(vec![value; 12], 12))
            .collect();
        let file = write_database(&[table(0, Vec::new()), table(1, rows)]).unwrap();
        let database = DatabaseFile::parse(&file).unwrap();
        assert_eq!(database.page_size, 4096);
        let empty = database.table(0).unwrap();
        assert_eq!(
            (empty.first_page, empty.last_page, empty.empty_candidate),
            (PageIndex(1), PageIndex(1), PageIndex(2))
        );
        assert_eq!(empty.pages[0].page_type, PageType::Index);
        let filled = database.table(1).unwrap();
        assert_eq!(
            (filled.first_page, filled.last_page),
            (PageIndex(3), PageIndex(4))
        );
        assert_eq!(filled.empty_candidate, PageIndex(5));
        assert_eq!(database.next_unused_page, PageIndex(6));
        assert_eq!(filled.rows, vec![vec![0; 12], vec![1; 12], vec![2; 12]]);
        let data = &filled.pages[1];
        assert_eq!(
            (data.slot_count, data.present_count, data.flags),
            (3, 3, 0x24)
        );
        assert_eq!(data.next_page, filled.empty_candidate);
        assert_eq!(usize::from(data.free_size), 4096 - 0x28 - 36 - (2 * 3 + 4));
    }

    #[test]
    fn rows_spill_onto_new_pages_with_their_place_in_each_page() {
        let rows = (0..400_u16)
            .map(|value| {
                let mut bytes = vec![0; 40];
                bytes[8..10].copy_from_slice(&value.to_le_bytes());
                EncodedRow::with_index_shift(bytes, 40)
            })
            .collect();
        let file = write_database(&[table(0, rows)]).unwrap();
        let database = DatabaseFile::parse(&file).unwrap();
        let tracks = database.table(0).unwrap();
        assert_eq!(tracks.rows.len(), 400);
        assert!(tracks.pages.len() > 3);
        for (number, row) in tracks.rows.iter().enumerate() {
            assert_eq!(get_u16(row, 8), Some(number as u16));
        }
        let first_page_rows = usize::from(tracks.pages[1].slot_count);
        assert_eq!(get_u16(&tracks.rows[1], 2), Some(0x20));
        assert_eq!(get_u16(&tracks.rows[first_page_rows], 2), Some(0));
        for page in &tracks.pages[1..] {
            let slots = usize::from(page.slot_count);
            let directory = slots.div_ceil(16) * 0x24;
            assert!(0x28 + usize::from(page.used_size) + directory <= 4096);
        }
    }

    #[test]
    fn a_row_bigger_than_a_page_is_refused() {
        let rows = vec![EncodedRow::new(vec![0; 4090], 4090)];
        assert!(matches!(
            write_database(&[table(0, rows)]),
            Err(DeviceLibraryError::RowTooLarge { .. })
        ));
    }

    #[test]
    fn a_chain_that_loops_is_an_error() {
        let mut file = write_database(&[table(0, vec![EncodedRow::new(vec![1; 8], 8)])]).unwrap();
        put_u32(&mut file[PAGE_SIZE * 2..], 0x0c, 1);
        put_u32(&mut file, FILE_HEADER_LENGTH + 12, 99);
        assert!(DatabaseFile::parse(&file).is_err());
    }
}
