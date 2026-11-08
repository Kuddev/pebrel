//! Read-only COM registration check; optional --invoke opens one requested folder.
use std::ffi::c_void;
use std::path::Path;
use windows::core::{implement, Interface, GUID, PCWSTR, PWSTR};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, IServiceProvider,
    IServiceProvider_Impl, CLSCTX_LOCAL_SERVER, COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::Ole::IObjectWithSite;
use windows::Win32::UI::Shell::{
    BHID_SFObject, IExplorerCommand, IFolderView, IShellFolder, IShellItem, IShellItemArray,
    SHCreateItemFromParsingName, SHCreateShellFolderView, SHCreateShellItemArrayFromShellItem,
    SFV_CREATE,
};

unsafe fn take_string(raw: PWSTR) -> windows::core::Result<String> {
    let text = raw.to_string();
    CoTaskMemFree(Some(raw.0 as *const c_void));
    Ok(text?)
}

// Exercise the no-selection branch with a real shell folder view supplied
// through the same service interface Explorer uses for a background menu.
#[implement(IServiceProvider)]
struct FolderSite(IFolderView);
impl IServiceProvider_Impl for FolderSite_Impl {
    fn QueryService(
        &self,
        service: *const GUID,
        riid: *const GUID,
        result: *mut *mut c_void,
    ) -> windows::core::Result<()> {
        use windows::Win32::Foundation::{E_NOINTERFACE, E_POINTER};
        if service.is_null() || riid.is_null() || result.is_null() {
            return Err(E_POINTER.into());
        }
        unsafe {
            *result = std::ptr::null_mut();
            if *service != IFolderView::IID {
                return Err(E_NOINTERFACE.into());
            }
            self.0.query(riid, result).ok()
        }
    }
}

fn main() -> windows::core::Result<()> {
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        let clsid = GUID::from_u128(0x209e5492_2c19_45c0_ae7e_a4beffd09073);
        {
            let command: IExplorerCommand = CoCreateInstance(&clsid, None, CLSCTX_LOCAL_SERVER)?;
            let title = take_string(command.GetTitle(None)?)?;
            let icon = take_string(command.GetIcon(None)?)?;
            assert!(!title.is_empty());
            assert!(Path::new(icon.strip_suffix(",0").expect("icon resource index")).is_file());
            assert_eq!(command.GetCanonicalName()?, clsid);
            println!("title = {title}\nicon = {icon}\nstate = {}", command.GetState(None, false)?);
            let mut args = std::env::args().skip(1);
            if let Some(option) = args.next() {
                assert!(matches!(option.as_str(), "--invoke" | "--background"));
                let path = args.next().expect("--invoke requires a directory");
                assert!(args.next().is_none());
                let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
                let item: IShellItem = SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None)?;
                if option == "--background" {
                    let folder: IShellFolder = item.BindToHandler(None, &BHID_SFObject)?;
                    let mut create = SFV_CREATE {
                        cbSize: std::mem::size_of::<SFV_CREATE>() as u32,
                        pshf: std::mem::ManuallyDrop::new(Some(folder)),
                        ..Default::default()
                    };
                    let view = SHCreateShellFolderView(&create);
                    std::mem::ManuallyDrop::drop(&mut create.pshf);
                    let site: IServiceProvider = FolderSite(view?.cast()?).into();
                    let with_site: IObjectWithSite = command.cast()?;
                    with_site.SetSite(&site)?;
                    let invoked = command.Invoke(None, None);
                    with_site.SetSite(None)?;
                    invoked?;
                } else {
                    let items: IShellItemArray = SHCreateShellItemArrayFromShellItem(&item)?;
                    command.Invoke(&items, None)?;
                }
                println!("invoked = {path}");
            }
        }
        CoUninitialize();
    }
    Ok(())
}
