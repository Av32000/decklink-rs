use crate::sdk;

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct DecklinkVideoInputFlags: u32 {
        const ENABLE_FORMAT_DETECTION = sdk::_DecklinkVideoInputFlags_decklinkVideoInputEnableFormatDetection;
        const DUAL_STREAM_3D = sdk::_DecklinkVideoInputFlags_decklinkVideoInputDualStream3D;
        const SYNCHRONIZE_TO_CAPTURE_GROUP = sdk::_DecklinkVideoInputFlags_decklinkVideoInputSynchronizeToCaptureGroup;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct DecklinkVideoInputFormatChangedEvents: u32 {
        const DISPLAY_MODE_CHANGED = sdk::_DecklinkVideoInputFormatChangedEvents_decklinkVideoInputDisplayModeChanged;
        const FIELD_DOMINANCE_CHANGED = sdk::_DecklinkVideoInputFormatChangedEvents_decklinkVideoInputFieldDominanceChanged;
        const COLORSPACE_CHANGED = sdk::_DecklinkVideoInputFormatChangedEvents_decklinkVideoInputColorspaceChanged;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct DecklinkDetectedVideoInputFormatFlags: u32 {
        const YCBCR_422 = sdk::_DecklinkDetectedVideoInputFormatFlags_decklinkDetectedVideoInputYCbCr422;
        const RGB_444 = sdk::_DecklinkDetectedVideoInputFormatFlags_decklinkDetectedVideoInputRGB444;
        const DUAL_STREAM_3D = sdk::_DecklinkDetectedVideoInputFormatFlags_decklinkDetectedVideoInputDualStream3D;
        const BIT_DEPTH_12 = sdk::_DecklinkDetectedVideoInputFormatFlags_decklinkDetectedVideoInput12BitDepth;
        const BIT_DEPTH_10 = sdk::_DecklinkDetectedVideoInputFormatFlags_decklinkDetectedVideoInput10BitDepth;
        const BIT_DEPTH_8 = sdk::_DecklinkDetectedVideoInputFormatFlags_decklinkDetectedVideoInput8BitDepth;
    }
}

#[derive(FromPrimitive, PartialEq, Debug, Copy, Clone)]
pub enum DecklinkAudioSampleRate {
    Rate48kHz = sdk::_DecklinkAudioSampleRate_decklinkAudioSampleRate48kHz as isize,
}

#[derive(FromPrimitive, PartialEq, Debug, Copy, Clone)]
pub enum DecklinkAudioSampleType {
    Int16 = sdk::_DecklinkAudioSampleType_decklinkAudioSampleType16bitInteger as isize,
    Int32 = sdk::_DecklinkAudioSampleType_decklinkAudioSampleType32bitInteger as isize,
}
