//! Images the WebView cannot decode, decoded by Windows (WIC, `docs/development-plan.md` §5.1):
//! HEIC / HEIF, through the HEIF Image Extensions from the Microsoft Store, which Windows uses
//! for Photos and Explorer's thumbnails too. Without them the decoder is missing and the page says
//! where to get it.

use std::path::Path;

use windows::{
    Win32::{
        Foundation::{GENERIC_READ, RPC_E_CHANGED_MODE},
        Graphics::Imaging::{
            CLSID_WICImagingFactory, GUID_WICPixelFormat32bppRGBA, IWICBitmapSource,
            IWICImagingFactory, WICBitmapDitherTypeNone, WICBitmapInterpolationModeFant,
            WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnDemand,
        },
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
            CoUninitialize,
            StructuredStorage::{PropVariantClear, PropVariantToUInt16},
        },
    },
    core::{HSTRING, Interface, w},
};

/// A decoded image: 8-bit RGBA rows, top to bottom.
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    /// EXIF orientation (1–8; 1 is upright), for the caller to apply.
    pub orientation: u16,
}

/// COM on this thread for the call (the protocol's threads have none).
struct Com(bool);

impl Com {
    fn new() -> Self {
        let r = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        // Already initialised another way: use it, and do not uninitialise it.
        Com(r.is_ok() && r != RPC_E_CHANGED_MODE)
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

/// Decodes the first frame of `path`, scaled down so neither side passes `max_side`.
pub fn decode(path: &Path, max_side: u32) -> windows::core::Result<Rgba> {
    let _com = Com::new();
    unsafe {
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let decoder = factory.CreateDecoderFromFilename(
            &HSTRING::from(path.as_os_str()),
            None,
            GENERIC_READ,
            WICDecodeMetadataCacheOnDemand,
        )?;
        let frame = decoder.GetFrame(0)?;
        let orientation = frame
            .GetMetadataQueryReader()
            .ok()
            .and_then(|q| {
                let mut v = Default::default();
                let r = q
                    .GetMetadataByName(w!("System.Photo.Orientation"), &mut v)
                    .ok()
                    .and_then(|_| PropVariantToUInt16(&v).ok());
                let _ = PropVariantClear(&mut v);
                r
            })
            .filter(|o| (1..=8).contains(o))
            .unwrap_or(1);
        let (mut w, mut h) = (0u32, 0u32);
        frame.GetSize(&mut w, &mut h)?;
        let mut source: IWICBitmapSource = frame.cast()?;
        if w.max(h) > max_side && w > 0 && h > 0 {
            let k = max_side as f64 / w.max(h) as f64;
            let (sw, sh) = (
                ((w as f64 * k) as u32).max(1),
                ((h as f64 * k) as u32).max(1),
            );
            let scaler = factory.CreateBitmapScaler()?;
            scaler.Initialize(&source, sw, sh, WICBitmapInterpolationModeFant)?;
            source = scaler.cast()?;
            (w, h) = (sw, sh);
        }
        let conv = factory.CreateFormatConverter()?;
        conv.Initialize(
            &source,
            &GUID_WICPixelFormat32bppRGBA,
            WICBitmapDitherTypeNone,
            None,
            0.0,
            WICBitmapPaletteTypeCustom,
        )?;
        let stride = w * 4;
        let mut pixels = vec![0u8; (stride * h) as usize];
        conv.CopyPixels(std::ptr::null(), stride, &mut pixels)?;
        Ok(Rgba {
            width: w,
            height: h,
            pixels,
            orientation,
        })
    }
}

#[cfg(test)]
mod tests {
    use windows::Win32::{
        Graphics::Imaging::{GUID_ContainerFormatHeif, WICBitmapEncoderNoCache},
        System::Com::{STGM_CREATE, STGM_WRITE},
        UI::Shell::SHCreateStreamOnFileEx,
    };

    use super::*;

    /// Writes a HEIC with Windows' own encoder (part of the HEIF extension); `false` when it is
    /// not installed.
    fn write_heic(path: &Path, w: u32, h: u32) -> bool {
        let _com = Com::new();
        unsafe {
            let Ok(factory) = CoCreateInstance::<_, IWICImagingFactory>(
                &CLSID_WICImagingFactory,
                None,
                CLSCTX_INPROC_SERVER,
            ) else {
                return false;
            };
            let Ok(enc) = factory.CreateEncoder(&GUID_ContainerFormatHeif, std::ptr::null()) else {
                return false;
            };
            let Ok(stream) = SHCreateStreamOnFileEx(
                &HSTRING::from(path.as_os_str()),
                (STGM_CREATE | STGM_WRITE).0,
                0,
                true,
                None,
            ) else {
                return false;
            };
            // A red-to-green gradient.
            let mut px = Vec::with_capacity((w * h * 4) as usize);
            for _y in 0..h {
                for x in 0..w {
                    px.extend_from_slice(&[(255 - x * 255 / w) as u8, (x * 255 / w) as u8, 0, 255]);
                }
            }
            let Ok(bitmap) =
                factory.CreateBitmapFromMemory(w, h, &GUID_WICPixelFormat32bppRGBA, w * 4, &px)
            else {
                return false;
            };
            let mut frame = None;
            let ok = enc.Initialize(&stream, WICBitmapEncoderNoCache).is_ok()
                && enc.CreateNewFrame(&mut frame, std::ptr::null_mut()).is_ok();
            let Some(frame) = frame.filter(|_| ok) else {
                return false;
            };
            frame.Initialize(None).is_ok()
                && frame.SetSize(w, h).is_ok()
                && frame.WriteSource(&bitmap, std::ptr::null()).is_ok()
                && frame.Commit().is_ok()
                && enc.Commit().is_ok()
        }
    }

    #[test]
    fn heic_round_trip_when_the_extension_is_there() {
        let d = std::env::temp_dir().join(format!("omnilook-wic-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("a.heic");
        if !write_heic(&p, 640, 320) {
            eprintln!("no HEIF encoder: skipped");
            return;
        }
        let img = decode(&p, 4096).unwrap();
        assert_eq!((img.width, img.height, img.orientation), (640, 320, 1));
        // Left edge red, right edge green (lossy: roughly).
        let at = |x: u32, y: u32| &img.pixels[((y * img.width + x) * 4) as usize..][..4];
        assert!(
            at(2, 160)[0] > 200 && at(2, 160)[1] < 60,
            "{:?}",
            at(2, 160)
        );
        assert!(
            at(637, 160)[1] > 200 && at(637, 160)[0] < 60,
            "{:?}",
            at(637, 160)
        );
        // Scaled down to fit.
        let small = decode(&p, 100).unwrap();
        assert_eq!((small.width, small.height), (100, 50));
        assert!(decode(&d.join("missing.heic"), 100).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }
}
