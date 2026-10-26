//! JSON C-ABI adapter for Harmony's shared annotation bridge.

use std::ffi::c_char;

use serde::de::DeserializeOwned;
use serde_json::Value;

use super::{c_str_to_rust, err_json, ok_json, with_app_service};
use crate::api::{
    AnnotationEditDto, AnnotationObjectRefDto, AnnotationTargetDto, EditorTransactionCauseDto,
};

fn field<'a>(args: &'a Value, name: &str) -> Result<&'a Value, String> {
    args.get(name)
        .ok_or_else(|| format!("annotation argument `{name}` is missing"))
}

fn string_arg(args: &Value, name: &str) -> Result<String, String> {
    field(args, name)?
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| format!("annotation argument `{name}` must be a string"))
}

fn decode_arg<T: DeserializeOwned>(args: &Value, name: &str) -> Result<T, String> {
    serde_json::from_value(field(args, name)?.clone())
        .map_err(|error| format!("invalid annotation argument `{name}`: {error}"))
}

fn unsigned_arg(args: &Value, name: &str) -> Result<u64, String> {
    field(args, name)?
        .as_u64()
        .ok_or_else(|| format!("annotation argument `{name}` must be an unsigned integer"))
}

fn annotation_call(operation: &str, args: Value) -> Result<Value, String> {
    with_app_service(|service| {
        macro_rules! call {
            ($expr:expr) => {
                $expr
                    .map(serde_json::to_value)
                    .map_err(|error| format!("{error}"))?
                    .map_err(|error| format!("could not encode annotation result: {error}"))
            };
        }
        let result = match operation {
            "listTags" => call!(service.list_tags(string_arg(&args, "projectId")?)),
            "createTag" => call!(service.create_tag(
                string_arg(&args, "projectId")?,
                string_arg(&args, "name")?,
                args.get("color")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            )),
            "renameTag" => call!(service.rename_tag(
                string_arg(&args, "projectId")?,
                string_arg(&args, "tagId")?,
                string_arg(&args, "name")?,
            )),
            "deleteTag" => {
                call!(service
                    .delete_tag(string_arg(&args, "projectId")?, string_arg(&args, "tagId")?,))
            }
            "mergeTags" => call!(service.merge_tags(
                string_arg(&args, "projectId")?,
                string_arg(&args, "sourceTagId")?,
                string_arg(&args, "targetTagId")?,
            )),
            "setTagBindings" => call!(service.set_tag_bindings(
                string_arg(&args, "projectId")?,
                decode_arg::<AnnotationObjectRefDto>(&args, "target")?,
                decode_arg::<Vec<String>>(&args, "tagIds")?,
            )),
            "listBindingsForTarget" => call!(service.list_bindings_for_target(
                string_arg(&args, "projectId")?,
                decode_arg::<AnnotationObjectRefDto>(&args, "target")?,
            )),
            "createTextAnchor" => call!(service.create_text_anchor(
                string_arg(&args, "projectId")?,
                string_arg(&args, "chapterId")?,
                u32::try_from(unsigned_arg(&args, "startByte")?)
                    .map_err(|error| format!("invalid startByte: {error}"))?,
                u32::try_from(unsigned_arg(&args, "endByte")?)
                    .map_err(|error| format!("invalid endByte: {error}"))?,
                unsigned_arg(&args, "textRevision")?,
                string_arg(&args, "text")?,
            )),
            "createNamedEntry" => call!(service.create_named_entry(
                string_arg(&args, "projectId")?,
                string_arg(&args, "canonicalName")?,
                decode_arg::<Vec<String>>(&args, "aliases")?,
                decode_arg::<Vec<String>>(&args, "confirmedMisspellings")?,
                field(&args, "autoLinkEnabled")?
                    .as_bool()
                    .ok_or_else(|| "autoLinkEnabled must be a boolean".to_string())?,
            )),
            "listNamedEntries" => {
                call!(service.list_named_entries(string_arg(&args, "projectId")?))
            }
            "updateNamedEntry" => call!(service.update_named_entry(
                string_arg(&args, "projectId")?,
                string_arg(&args, "entryId")?,
                string_arg(&args, "canonicalName")?,
                decode_arg::<Vec<String>>(&args, "aliases")?,
                decode_arg::<Vec<String>>(&args, "confirmedMisspellings")?,
                field(&args, "autoLinkEnabled")?
                    .as_bool()
                    .ok_or_else(|| "autoLinkEnabled must be a boolean".to_string())?,
            )),
            "deleteNamedEntry" => call!(service.delete_named_entry(
                string_arg(&args, "projectId")?,
                string_arg(&args, "entryId")?,
            )),
            "createLink" => call!(service.create_link(
                decode_arg::<AnnotationObjectRefDto>(&args, "source")?,
                decode_arg::<AnnotationTargetDto>(&args, "target")?,
            )),
            "updateLink" => call!(service.update_link(
                string_arg(&args, "projectId")?,
                string_arg(&args, "linkId")?,
                decode_arg::<AnnotationObjectRefDto>(&args, "source")?,
                decode_arg::<AnnotationTargetDto>(&args, "target")?,
            )),
            "deleteLink" => call!(service.delete_link(
                string_arg(&args, "projectId")?,
                string_arg(&args, "linkId")?,
            )),
            "resolveLink" => {
                call!(service.resolve_link(decode_arg::<AnnotationTargetDto>(&args, "target")?,))
            }
            "findBacklinks" => {
                call!(service.find_backlinks(decode_arg::<AnnotationTargetDto>(&args, "target")?,))
            }
            "listLinksForSource" => {
                call!(service
                    .list_links_for_source(decode_arg::<AnnotationObjectRefDto>(&args, "source")?,))
            }
            "getChapterAnnotations" => call!(service.get_chapter_annotations(
                string_arg(&args, "projectId")?,
                string_arg(&args, "chapterId")?,
                unsigned_arg(&args, "textRevision")?,
                string_arg(&args, "text")?,
            )),
            "applyAnnotationEdits" => {
                let cause: EditorTransactionCauseDto = decode_arg(&args, "cause")?;
                call!(service.apply_annotation_edits(
                    string_arg(&args, "projectId")?,
                    string_arg(&args, "chapterId")?,
                    unsigned_arg(&args, "baseRevision")?,
                    unsigned_arg(&args, "textRevision")?,
                    cause,
                    decode_arg::<Vec<AnnotationEditDto>>(&args, "edits")?,
                ))
            }
            "findAutoNameLinks" => call!(service.find_auto_name_links(
                string_arg(&args, "projectId")?,
                string_arg(&args, "chapterId")?,
                unsigned_arg(&args, "textRevision")?,
                string_arg(&args, "text")?,
            )),
            _ => return Err(format!("unknown annotation operation `{operation}`")),
        };
        Ok(result)
    })
}

/// Dispatch one typed annotation call from the Harmony NAPI bridge.
///
/// # Safety
/// Both arguments must be valid null-terminated UTF-8 strings.
#[no_mangle]
pub unsafe extern "C" fn writer_core_annotation_call(
    operation: *const c_char,
    args_json: *const c_char,
) -> *mut c_char {
    let operation = match c_str_to_rust(operation) {
        Ok(value) => value,
        Err(error) => return err_json("INVALID_ARGUMENT", &format!("invalid operation: {error}")),
    };
    let args_json = match c_str_to_rust(args_json) {
        Ok(value) => value,
        Err(error) => return err_json("INVALID_ARGUMENT", &format!("invalid arguments: {error}")),
    };
    let args = match serde_json::from_str::<Value>(&args_json) {
        Ok(value) if value.is_object() => value,
        Ok(_) => {
            return err_json(
                "INVALID_ARGUMENT",
                "annotation arguments must be a JSON object",
            )
        }
        Err(error) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("invalid JSON arguments: {error}"),
            )
        }
    };
    match annotation_call(&operation, args) {
        Ok(value) => ok_json(value),
        Err(error) => err_json("ANNOTATION_ERROR", &error),
    }
}
