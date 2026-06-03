use crate::{
    ion::IonResult,
    mzml::structs::{Chromatogram, MzML, Spectrum},
};

pub trait FileReader {
    fn get_metadata(&mut self) -> IonResult<MzML>;
    fn next_spectrum(&mut self) -> IonResult<Option<Spectrum>>;
    fn next_chromatogram(&mut self) -> IonResult<Option<Chromatogram>>;
}

pub struct MemoryReader {
    metadata: MzML,
    spectra: std::vec::IntoIter<Spectrum>,
    chromatograms: std::vec::IntoIter<Chromatogram>,
}

impl MemoryReader {
    pub fn new(mut mzml: MzML) -> Self {
        let spectra = mzml
            .run
            .spectrum_list
            .as_mut()
            .map(|list| std::mem::take(&mut list.spectra))
            .unwrap_or_default();
        let chromatograms = mzml
            .run
            .chromatogram_list
            .as_mut()
            .map(|list| std::mem::take(&mut list.chromatograms))
            .unwrap_or_default();
        Self {
            metadata: mzml,
            spectra: spectra.into_iter(),
            chromatograms: chromatograms.into_iter(),
        }
    }
}

impl FileReader for MemoryReader {
    fn get_metadata(&mut self) -> IonResult<MzML> {
        Ok(self.metadata.clone())
    }

    fn next_spectrum(&mut self) -> IonResult<Option<Spectrum>> {
        Ok(self.spectra.next())
    }

    fn next_chromatogram(&mut self) -> IonResult<Option<Chromatogram>> {
        Ok(self.chromatograms.next())
    }
}
