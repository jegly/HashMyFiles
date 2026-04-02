use super::dialog::AdwDialogImpl;
use crate::PreferencesDialog;
use glib::subclass::prelude::*;

pub trait PreferencesDialogImpl: AdwDialogImpl {}

unsafe impl<T: PreferencesDialogImpl> IsSubclassable<T> for PreferencesDialog {}
