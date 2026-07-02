use qmetaobject::{QJsonArray, QJsonObject, QJsonValue, QString};

pub(crate) fn envelope_error_json(error: writer_core::api::WriterError) -> String {
    writer_core::api::ResultEnvelope::<serde_json::Value>::error(error).to_json_string()
}

pub(crate) fn envelope_ok_json<T: serde::Serialize>(data: T) -> String {
    writer_core::api::ResultEnvelope::success(data).to_json_string()
}

pub(crate) fn serde_value_to_qjson(value: serde_json::Value) -> QJsonValue {
    match value {
        serde_json::Value::Null => QJsonValue::default(),
        serde_json::Value::Bool(v) => QJsonValue::from(v),
        serde_json::Value::Number(v) => QJsonValue::from(v.as_f64().unwrap_or_default()),
        serde_json::Value::String(v) => QJsonValue::from(QString::from(v)),
        serde_json::Value::Array(values) => {
            let mut arr = QJsonArray::default();
            for item in values {
                arr.push(serde_value_to_qjson(item));
            }
            QJsonValue::from(arr)
        }
        serde_json::Value::Object(values) => {
            let mut obj = QJsonObject::default();
            for (key, item) in values {
                obj.insert(&key, serde_value_to_qjson(item));
            }
            QJsonValue::from(obj)
        }
    }
}

pub(crate) fn serde_to_qjson_object(value: serde_json::Value) -> QJsonObject {
    if let serde_json::Value::Object(values) = value {
        let mut obj = QJsonObject::default();
        for (key, item) in values {
            obj.insert(&key, serde_value_to_qjson(item));
        }
        obj
    } else {
        QJsonObject::default()
    }
}

pub(crate) fn serde_to_qjson_array(value: serde_json::Value) -> QJsonArray {
    if let serde_json::Value::Array(values) = value {
        let mut arr = QJsonArray::default();
        for item in values {
            arr.push(serde_value_to_qjson(item));
        }
        arr
    } else {
        QJsonArray::default()
    }
}

pub(crate) fn bridge_error_object(message_key: &str, code: &str, raw_error: &str) -> QJsonObject {
    // messageKey 供 QML 侧做 qsTr 翻译，rawError 放技术细节
    // 不再输出 userMessage，用户可见文案由 QML 侧根据 messageKey 翻译
    serde_to_qjson_object(serde_json::json!({
        "success": false,
        "errorCode": code,
        "messageKey": message_key,
        "messageArgs": {},
        "rawError": raw_error,
        "warnings": [],
        "changedPaths": [],
        "changedEntities": []
    }))
}

pub(crate) fn bridge_success_object(data: serde_json::Value) -> QJsonObject {
    serde_to_qjson_object(serde_json::json!({
        "success": true,
        "data": data,
        "warnings": [],
        "changedPaths": [],
        "changedEntities": []
    }))
}

pub(crate) fn qjson_object_from_json(raw: &str) -> QJsonObject {
    match serde_json::from_str::<serde_json::Value>(raw) {
        Ok(value) => serde_to_qjson_object(value),
        Err(e) => bridge_error_object(
            "error.json_parse",
            "JSON_ERROR",
            &format!("无效 Bridge 返回: {}", e),
        ),
    }
}

pub(crate) fn qjson_array_data_from_json(raw: &str) -> QJsonArray {
    match serde_json::from_str::<serde_json::Value>(raw) {
        Ok(value) => serde_to_qjson_array(
            value
                .get("data")
                .cloned()
                .unwrap_or(serde_json::Value::Array(vec![])),
        ),
        Err(_) => QJsonArray::default(),
    }
}
