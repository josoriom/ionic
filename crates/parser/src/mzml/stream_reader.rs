#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
use std::{
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
};

#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
use quick_xml::{
    Reader,
    events::{BytesStart, Event},
};

#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
use crate::{
    ion::IonError,
    mzml::{
        schema::TagId,
        structs::{
            ChromatogramList, DataProcessingList, InstrumentList, ReferenceableParamGroupList, Run,
            SampleList, ScanSettingsList, SoftwareList, SourceFileRefList, SpectrumList,
        },
        utilities::{
            ParamCollector, ParseError, ParsingWorkspace, attr, attr_u32, attr_usize,
            drain_until_close, finalize_bda, parse_chromatogram_list::parse_chromatogram,
            parse_cv_list, parse_data_processing_list, parse_file_description,
            parse_instrument_list, parse_ref_param_group_list, parse_sample_list,
            parse_scan_settings_list, parse_software_list, parse_source_file_ref_list,
            parse_spectrum_list::parse_spectrum, read_cv_param, read_ref_group_ref,
            read_user_param, tag_id_from_bytes,
        },
    },
};
use crate::{
    ion::{
        IonResult,
        encoder::scan_stream::{MemoryReader, ScanStream},
    },
    mzml::structs::{Chromatogram, MzML, Spectrum},
};

pub struct MzmlReader {
    kind: ReaderKind,
}

enum ReaderKind {
    #[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
    File(StreamReader),
    Memory(MemoryReader),
}

#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
struct StreamReader {
    workspace: ParsingWorkspace<BufReader<File>>,
    path: PathBuf,
    metadata: MzML,
    state: ReaderState,
}

#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
#[derive(Clone, Copy, PartialEq, Eq)]
enum ReaderState {
    NeedMetadata,
    SpectrumList,
    NeedChromatograms,
    ChromatogramList,
    NeedRunEnd,
    NeedMzmlEnd,
    Done,
}

impl MzmlReader {
    #[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
    pub fn open(path: &Path) -> IonResult<Self> {
        let file = File::open(path).map_err(|err| {
            IonError::from(format!("cannot open mzML file '{}': {err}", path.display()))
        })?;
        let reader = Reader::from_reader(BufReader::new(file));
        Ok(Self {
            kind: ReaderKind::File(StreamReader {
                workspace: ParsingWorkspace::new(reader),
                path: path.to_path_buf(),
                metadata: MzML::default(),
                state: ReaderState::NeedMetadata,
            }),
        })
    }

    pub fn from_mzml(mzml: MzML) -> Self {
        Self {
            kind: ReaderKind::Memory(MemoryReader::new(mzml)),
        }
    }

    #[cfg(all(target_arch = "wasm32", not(target_os = "wasi")))]
    pub fn open(_path: &std::path::Path) -> IonResult<Self> {
        Err("MzmlReader::open is not available in browser wasm".into())
    }
}

impl ScanStream for MzmlReader {
    fn metadata(&mut self) -> IonResult<MzML> {
        match &mut self.kind {
            #[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
            ReaderKind::File(reader) => reader.metadata(),
            ReaderKind::Memory(reader) => reader.metadata(),
        }
    }

    fn next_spectrum(&mut self) -> IonResult<Option<Spectrum>> {
        match &mut self.kind {
            #[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
            ReaderKind::File(reader) => reader.next_spectrum(),
            ReaderKind::Memory(reader) => reader.next_spectrum(),
        }
    }

    fn next_chromatogram(&mut self) -> IonResult<Option<Chromatogram>> {
        match &mut self.kind {
            #[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
            ReaderKind::File(reader) => reader.next_chromatogram(),
            ReaderKind::Memory(reader) => reader.next_chromatogram(),
        }
    }
}

#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
impl StreamReader {
    fn metadata(&mut self) -> IonResult<MzML> {
        match self.state {
            ReaderState::NeedMetadata => self.read_metadata()?,
            ReaderState::NeedChromatograms => self.read_to_chromatograms()?,
            ReaderState::NeedRunEnd | ReaderState::NeedMzmlEnd => self.read_to_end()?,
            _ => {}
        }
        Ok(self.metadata.clone())
    }

    fn next_spectrum(&mut self) -> IonResult<Option<Spectrum>> {
        if self.state == ReaderState::NeedMetadata {
            self.metadata()?;
        }
        if self.state != ReaderState::SpectrumList {
            return Ok(None);
        }
        loop {
            match self.next_event()? {
                Event::Start(element) => match tag_id_from_bytes(element.name().as_ref()) {
                    TagId::Spectrum => {
                        let mut spectrum = parse_or_error(
                            &self.path,
                            parse_spectrum(&mut self.workspace, &element),
                        )?;
                        finish_spectrum_arrays(&mut spectrum).map_err(|err| self.error(err))?;
                        return Ok(Some(spectrum));
                    }
                    _ => drain_until_close(&mut self.workspace, element.name().as_ref())
                        .map_err(|err| self.error(err))?,
                },
                Event::Empty(element) => {
                    if tag_id_from_bytes(element.name().as_ref()) == TagId::Spectrum {
                        return Ok(Some(Spectrum {
                            id: attr(&element, b"id").unwrap_or_default(),
                            index: attr_u32(&element, b"index"),
                            ..Default::default()
                        }));
                    }
                }
                Event::End(element)
                    if tag_id_from_bytes(element.name().as_ref()) == TagId::SpectrumList =>
                {
                    self.state = ReaderState::NeedChromatograms;
                    return Ok(None);
                }
                Event::Eof => return Err(self.unexpected_end("spectrumList")),
                _ => {}
            }
        }
    }

    fn next_chromatogram(&mut self) -> IonResult<Option<Chromatogram>> {
        if self.state == ReaderState::NeedMetadata || self.state == ReaderState::NeedChromatograms {
            self.metadata()?;
        }
        if self.state != ReaderState::ChromatogramList {
            return Ok(None);
        }
        loop {
            match self.next_event()? {
                Event::Start(element) => match tag_id_from_bytes(element.name().as_ref()) {
                    TagId::Chromatogram => {
                        let mut chromatogram = parse_or_error(
                            &self.path,
                            parse_chromatogram(&mut self.workspace, &element),
                        )?;
                        finish_chromatogram_arrays(&mut chromatogram)
                            .map_err(|err| self.error(err))?;
                        return Ok(Some(chromatogram));
                    }
                    _ => drain_until_close(&mut self.workspace, element.name().as_ref())
                        .map_err(|err| self.error(err))?,
                },
                Event::Empty(element) => {
                    if tag_id_from_bytes(element.name().as_ref()) == TagId::Chromatogram {
                        return Ok(Some(Chromatogram {
                            id: attr(&element, b"id").unwrap_or_default(),
                            index: attr_u32(&element, b"index"),
                            ..Default::default()
                        }));
                    }
                }
                Event::End(element)
                    if tag_id_from_bytes(element.name().as_ref()) == TagId::ChromatogramList =>
                {
                    self.state = ReaderState::NeedRunEnd;
                    return Ok(None);
                }
                Event::Eof => return Err(self.unexpected_end("chromatogramList")),
                _ => {}
            }
        }
    }

    fn read_metadata(&mut self) -> IonResult<()> {
        let mut inside_mzml = false;
        loop {
            match self.next_event()? {
                Event::Start(element) => {
                    let tag = tag_id_from_bytes(element.name().as_ref());
                    if !inside_mzml {
                        if tag == TagId::MzML {
                            inside_mzml = true;
                        }
                        continue;
                    }
                    match tag {
                        TagId::CvList => {
                            self.metadata.cv_list = Some(parse_or_error(
                                &self.path,
                                parse_cv_list(&mut self.workspace, &element),
                            )?);
                        }
                        TagId::FileDescription => {
                            self.metadata.file_description = Some(parse_or_error(
                                &self.path,
                                parse_file_description(&mut self.workspace, &element),
                            )?);
                        }
                        TagId::ReferenceableParamGroupList => {
                            self.metadata.referenceable_param_group_list = Some(parse_or_error(
                                &self.path,
                                parse_ref_param_group_list(&mut self.workspace, &element),
                            )?);
                        }
                        TagId::SampleList => {
                            self.metadata.sample_list = Some(parse_or_error(
                                &self.path,
                                parse_sample_list(&mut self.workspace, &element),
                            )?);
                        }
                        TagId::InstrumentConfigurationList => {
                            self.metadata.instrument_list = parse_or_error(
                                &self.path,
                                parse_instrument_list(&mut self.workspace, &element),
                            )?;
                        }
                        TagId::SoftwareList => {
                            self.metadata.software_list = Some(parse_or_error(
                                &self.path,
                                parse_software_list(&mut self.workspace, &element),
                            )?);
                        }
                        TagId::DataProcessingList => {
                            self.metadata.data_processing_list = Some(parse_or_error(
                                &self.path,
                                parse_data_processing_list(&mut self.workspace, &element),
                            )?);
                        }
                        TagId::ScanSettingsList | TagId::AcquisitionSettingsList => {
                            self.metadata.scan_settings_list = parse_or_error(
                                &self.path,
                                parse_scan_settings_list(&mut self.workspace, &element),
                            )?;
                        }
                        TagId::Run => {
                            self.read_run(&element)?;
                            return Ok(());
                        }
                        _ => drain_until_close(&mut self.workspace, element.name().as_ref())
                            .map_err(|err| self.error(err))?,
                    }
                }
                Event::Empty(element) => {
                    if inside_mzml {
                        self.read_empty_metadata(&element);
                    }
                }
                Event::End(element)
                    if tag_id_from_bytes(element.name().as_ref()) == TagId::MzML =>
                {
                    self.state = ReaderState::Done;
                    return Ok(());
                }
                Event::Eof => {
                    if inside_mzml {
                        return Err(self.unexpected_end("mzML"));
                    }
                    self.state = ReaderState::Done;
                    return Ok(());
                }
                _ => {}
            }
        }
    }

    fn read_empty_metadata(&mut self, element: &BytesStart<'_>) {
        match tag_id_from_bytes(element.name().as_ref()) {
            TagId::ReferenceableParamGroupList => {
                self.metadata.referenceable_param_group_list = Some(ReferenceableParamGroupList {
                    count: attr_usize(element, b"count"),
                    ..Default::default()
                });
            }
            TagId::SampleList => {
                self.metadata.sample_list = Some(SampleList {
                    count: attr_u32(element, b"count"),
                    ..Default::default()
                });
            }
            TagId::InstrumentConfigurationList => {
                self.metadata.instrument_list = Some(InstrumentList {
                    count: attr_usize(element, b"count"),
                    ..Default::default()
                });
            }
            TagId::SoftwareList => {
                self.metadata.software_list = Some(SoftwareList {
                    count: attr_usize(element, b"count"),
                    ..Default::default()
                });
            }
            TagId::DataProcessingList => {
                self.metadata.data_processing_list = Some(DataProcessingList {
                    count: attr_usize(element, b"count"),
                    ..Default::default()
                });
            }
            TagId::ScanSettingsList | TagId::AcquisitionSettingsList => {
                self.metadata.scan_settings_list = Some(ScanSettingsList {
                    count: attr_usize(element, b"count"),
                    ..Default::default()
                });
            }
            _ => {}
        }
    }

    fn read_run(&mut self, start: &BytesStart<'_>) -> IonResult<()> {
        self.metadata.run = Run {
            id: attr(start, b"id").unwrap_or_default(),
            start_time_stamp: attr(start, b"startTimeStamp"),
            default_instrument_configuration_ref: attr(start, b"defaultInstrumentConfigurationRef")
                .or_else(|| attr(start, b"instrumentRef")),
            default_source_file_ref: attr(start, b"defaultSourceFileRef"),
            sample_ref: attr(start, b"sampleRef"),
            ..Default::default()
        };
        self.read_run_body()
    }

    fn read_run_body(&mut self) -> IonResult<()> {
        loop {
            match self.next_event()? {
                Event::Start(element) => match tag_id_from_bytes(element.name().as_ref()) {
                    TagId::CvParam => self.read_run_param(&element, RunParam::Cv)?,
                    TagId::UserParam => self.read_run_param(&element, RunParam::User)?,
                    TagId::ReferenceableParamGroupRef => {
                        self.read_run_param(&element, RunParam::Reference)?;
                    }
                    TagId::SourceFileRefList => {
                        self.metadata.run.source_file_ref_list = Some(parse_or_error(
                            &self.path,
                            parse_source_file_ref_list(&mut self.workspace, &element),
                        )?);
                    }
                    TagId::SpectrumList => {
                        self.metadata.run.spectrum_list = Some(get_spectrum_list(&element));
                        self.state = ReaderState::SpectrumList;
                        return Ok(());
                    }
                    TagId::ChromatogramList => {
                        self.metadata.run.chromatogram_list = Some(get_chromatogram_list(&element));
                        self.state = ReaderState::ChromatogramList;
                        return Ok(());
                    }
                    _ => drain_until_close(&mut self.workspace, element.name().as_ref())
                        .map_err(|err| self.error(err))?,
                },
                Event::Empty(element) => match tag_id_from_bytes(element.name().as_ref()) {
                    TagId::CvParam => self.metadata.run.receive_cv(read_cv_param(&element)),
                    TagId::UserParam => self.metadata.run.receive_user(read_user_param(&element)),
                    TagId::ReferenceableParamGroupRef => self
                        .metadata
                        .run
                        .receive_ref_group(read_ref_group_ref(&element)),
                    TagId::SourceFileRefList => {
                        self.metadata.run.source_file_ref_list = Some(SourceFileRefList {
                            count: attr_usize(&element, b"count"),
                            ..Default::default()
                        });
                    }
                    TagId::SpectrumList => {
                        self.metadata.run.spectrum_list = Some(get_spectrum_list(&element));
                        self.state = ReaderState::NeedChromatograms;
                        return Ok(());
                    }
                    TagId::ChromatogramList => {
                        self.metadata.run.chromatogram_list = Some(get_chromatogram_list(&element));
                        self.state = ReaderState::NeedRunEnd;
                        return Ok(());
                    }
                    _ => {}
                },
                Event::End(element) if tag_id_from_bytes(element.name().as_ref()) == TagId::Run => {
                    self.state = ReaderState::NeedMzmlEnd;
                    return Ok(());
                }
                Event::Eof => return Err(self.unexpected_end("run")),
                _ => {}
            }
        }
    }

    fn read_to_chromatograms(&mut self) -> IonResult<()> {
        loop {
            match self.next_event()? {
                Event::Start(element) => match tag_id_from_bytes(element.name().as_ref()) {
                    TagId::ChromatogramList => {
                        self.metadata.run.chromatogram_list = Some(get_chromatogram_list(&element));
                        self.state = ReaderState::ChromatogramList;
                        return Ok(());
                    }
                    _ => drain_until_close(&mut self.workspace, element.name().as_ref())
                        .map_err(|err| self.error(err))?,
                },
                Event::Empty(element) => {
                    if tag_id_from_bytes(element.name().as_ref()) == TagId::ChromatogramList {
                        self.metadata.run.chromatogram_list = Some(get_chromatogram_list(&element));
                        self.state = ReaderState::NeedRunEnd;
                        return Ok(());
                    }
                }
                Event::End(element) if tag_id_from_bytes(element.name().as_ref()) == TagId::Run => {
                    self.state = ReaderState::NeedMzmlEnd;
                    return Ok(());
                }
                Event::Eof => return Err(self.unexpected_end("run")),
                _ => {}
            }
        }
    }

    fn read_to_end(&mut self) -> IonResult<()> {
        if self.state == ReaderState::NeedRunEnd {
            loop {
                match self.next_event()? {
                    Event::End(element)
                        if tag_id_from_bytes(element.name().as_ref()) == TagId::Run =>
                    {
                        self.state = ReaderState::NeedMzmlEnd;
                        break;
                    }
                    Event::Start(element) => {
                        drain_until_close(&mut self.workspace, element.name().as_ref())
                            .map_err(|err| self.error(err))?
                    }
                    Event::Eof => return Err(self.unexpected_end("run")),
                    _ => {}
                }
            }
        }
        if self.state == ReaderState::NeedMzmlEnd {
            loop {
                match self.next_event()? {
                    Event::End(element)
                        if tag_id_from_bytes(element.name().as_ref()) == TagId::MzML =>
                    {
                        self.state = ReaderState::Done;
                        return Ok(());
                    }
                    Event::Start(element) => {
                        drain_until_close(&mut self.workspace, element.name().as_ref())
                            .map_err(|err| self.error(err))?
                    }
                    Event::Eof => return Err(self.unexpected_end("mzML")),
                    _ => {}
                }
            }
        }
        Ok(())
    }

    fn read_run_param(&mut self, element: &BytesStart<'_>, param: RunParam) -> IonResult<()> {
        match param {
            RunParam::Cv => self.metadata.run.receive_cv(read_cv_param(element)),
            RunParam::User => self.metadata.run.receive_user(read_user_param(element)),
            RunParam::Reference => self
                .metadata
                .run
                .receive_ref_group(read_ref_group_ref(element)),
        }
        drain_until_close(&mut self.workspace, element.name().as_ref())
            .map_err(|err| self.error(err))
    }

    fn next_event(&mut self) -> IonResult<Event<'static>> {
        self.workspace.next_event().map_err(|err| self.error(err))
    }

    fn error(&self, error: ParseError) -> IonError {
        IonError::from(format!(
            "cannot parse mzML file '{}': {error}",
            self.path.display()
        ))
    }

    fn unexpected_end(&self, context: &str) -> IonError {
        IonError::from(format!(
            "cannot parse mzML file '{}': unexpected end of file in {context}",
            self.path.display()
        ))
    }
}

#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
enum RunParam {
    Cv,
    User,
    Reference,
}

#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
fn get_spectrum_list(element: &BytesStart<'_>) -> SpectrumList {
    SpectrumList {
        count: attr_usize(element, b"count"),
        default_data_processing_ref: attr(element, b"defaultDataProcessingRef"),
        ..Default::default()
    }
}

#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
fn get_chromatogram_list(element: &BytesStart<'_>) -> ChromatogramList {
    ChromatogramList {
        count: attr_usize(element, b"count"),
        default_data_processing_ref: attr(element, b"defaultDataProcessingRef"),
        ..Default::default()
    }
}

#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
fn parse_or_error<T>(path: &Path, result: Result<T, ParseError>) -> IonResult<T> {
    result.map_err(|error| {
        IonError::from(format!(
            "cannot parse mzML file '{}': {error}",
            path.display()
        ))
    })
}

#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
fn finish_spectrum_arrays(spectrum: &mut Spectrum) -> Result<(), ParseError> {
    if let Some(list) = spectrum.binary_data_array_list.as_mut() {
        for array in &mut list.binary_data_arrays {
            finalize_bda(array, spectrum.default_array_length)?;
        }
    }
    Ok(())
}

#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
fn finish_chromatogram_arrays(chromatogram: &mut Chromatogram) -> Result<(), ParseError> {
    if let Some(list) = chromatogram.binary_data_array_list.as_mut() {
        for array in &mut list.binary_data_arrays {
            finalize_bda(array, chromatogram.default_array_length)?;
        }
    }
    Ok(())
}
