use std::fs::File;
use std::path::Path;

use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::packet::Packet;
use symphonia::core::units::TimeBase;

use crate::error::EngineError;
use crate::track::{Track, TrackWriter, check_sample_rate};

const PUBLISH_EVERY_FRAMES: usize = 4096;

/// Opens an audio file and starts decoding it on a background thread.
///
/// The file is opened and its format checked before this returns, so an unreadable or
/// unsupported file is reported here. The returned track fills up as the decoder runs, much
/// faster than real time; it can be loaded on a deck and played straight away. MP3, AAC and ALAC
/// in M4A, FLAC, WAV, AIFF, Ogg Vorbis and Matroska audio are supported.
pub fn decode_file(path: impl AsRef<Path>) -> Result<Track, EngineError> {
    let path = path.as_ref().to_path_buf();
    let session = DecodeSession::open(&path)?;
    let track = session.writer.track();
    std::thread::Builder::new()
        .name("dubplate-decode".to_owned())
        .spawn(move || session.run())
        .map_err(|error| EngineError::Decode {
            path,
            message: format!("could not start the decoding thread: {error}"),
        })?;
    Ok(track)
}

struct DecodeSession {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    time_base: Option<TimeBase>,
    sample_rate: u32,
    writer: TrackWriter,
    interleaved: Vec<f32>,
}

impl DecodeSession {
    fn open(path: &Path) -> Result<Self, EngineError> {
        let file = File::open(path).map_err(|source| EngineError::OpenFile {
            path: path.to_path_buf(),
            source,
        })?;
        let decode_error = |message: String| EngineError::Decode {
            path: path.to_path_buf(),
            message,
        };
        let mut hint = Hint::new();
        if let Some(extension) = path.extension().and_then(|extension| extension.to_str()) {
            hint.with_extension(extension);
        }
        let stream = MediaSourceStream::new(Box::new(file), Default::default());
        let format = symphonia::default::get_probe()
            .probe(
                &hint,
                stream,
                FormatOptions::default(),
                MetadataOptions::default(),
            )
            .map_err(|error| match error {
                SymphoniaError::Unsupported(_) => {
                    decode_error("the file is not in an audio format Dubplate reads".to_owned())
                }
                other => decode_error(other.to_string()),
            })?;
        let audio_track = format
            .default_track(TrackType::Audio)
            .ok_or_else(|| decode_error("the file has no audio track".to_owned()))?;
        let parameters = audio_track
            .codec_params
            .as_ref()
            .and_then(|parameters| parameters.audio())
            .ok_or_else(|| decode_error("the audio track has no codec parameters".to_owned()))?;
        let sample_rate = parameters
            .sample_rate
            .ok_or_else(|| decode_error("the audio track has no sample rate".to_owned()))?;
        check_sample_rate(sample_rate).map_err(|_| {
            decode_error(format!(
                "the sample rate of {sample_rate} Hz is not supported"
            ))
        })?;
        let source_channels = parameters
            .channels
            .as_ref()
            .map_or(2, |channels| channels.count())
            .try_into()
            .unwrap_or(u16::MAX);
        let decoder = symphonia::default::get_codecs()
            .make_audio_decoder(parameters, &AudioDecoderOptions::default())
            .map_err(|error| decode_error(error.to_string()))?;
        let track_id = audio_track.id;
        let time_base = audio_track.time_base;
        Ok(Self {
            format,
            decoder,
            track_id,
            time_base,
            sample_rate,
            writer: TrackWriter::new(sample_rate, source_channels),
            interleaved: Vec::new(),
        })
    }

    fn run(mut self) {
        let mut unpublished_frames = 0;
        loop {
            if self.writer.is_abandoned() {
                self.writer
                    .fail("nothing holds the track any more".to_owned());
                return;
            }
            let packet = match self.format.next_packet() {
                Ok(Some(packet)) => packet,
                Ok(None) => break,
                Err(SymphoniaError::IoError(error))
                    if error.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    break;
                }
                Err(error) => {
                    self.writer.fail(error.to_string());
                    return;
                }
            };
            if packet.track_id != self.track_id {
                continue;
            }
            let written = match self.decode_packet(&packet) {
                Ok(written) => written,
                Err(message) => {
                    self.writer.fail(message);
                    return;
                }
            };
            unpublished_frames += written;
            if unpublished_frames >= PUBLISH_EVERY_FRAMES {
                self.writer.publish();
                unpublished_frames = 0;
            }
        }
        self.writer.finish();
    }

    /// Decodes one packet into the track and returns the frames written. A packet with corrupt
    /// data becomes silence of the same length, so the rest of the track keeps its timing.
    fn decode_packet(&mut self, packet: &Packet) -> Result<usize, String> {
        match self.decoder.decode(packet) {
            Ok(decoded) => {
                let channel_count = decoded.spec().channels().count().max(1);
                self.interleaved.resize(decoded.samples_interleaved(), 0.0);
                decoded.copy_to_slice_interleaved(&mut self.interleaved);
                let mut written = 0;
                for frame in self.interleaved.chunks_exact(channel_count) {
                    let left = frame[0];
                    let right = frame.get(1).copied().unwrap_or(left);
                    if !self.writer.push_frame(left, right) {
                        return Err(too_long_message());
                    }
                    written += 1;
                }
                Ok(written)
            }
            Err(SymphoniaError::DecodeError(_) | SymphoniaError::IoError(_)) => {
                let silent_frames = self.packet_frames(packet);
                for _ in 0..silent_frames {
                    if !self.writer.push_frame(0.0, 0.0) {
                        return Err(too_long_message());
                    }
                }
                Ok(silent_frames)
            }
            Err(error) => Err(error.to_string()),
        }
    }

    fn packet_frames(&self, packet: &Packet) -> usize {
        let sample_rate = f64::from(self.sample_rate);
        match self.time_base {
            Some(time_base) => time_base.calc_duration(packet.dur).map_or(0, |duration| {
                (duration.as_secs_f64() * sample_rate).round() as usize
            }),
            None => usize::try_from(packet.dur.get()).unwrap_or(0),
        }
    }
}

fn too_long_message() -> String {
    "the track is longer than the four hours a deck can hold".to_owned()
}
