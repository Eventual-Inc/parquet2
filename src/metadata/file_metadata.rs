use crate::{error::Error, metadata::get_sort_order};

use super::{column_order::ColumnOrder, schema_descriptor::SchemaDescriptor, RowGroupMetaData};
use parquet_format_safe::ColumnOrder as TColumnOrder;
#[cfg(feature = "serde_types")]
use serde::{Deserialize, Serialize};

pub use crate::thrift_format::KeyValue;

#[cfg(feature = "serde_types")]
pub mod key_value_serde {
    use super::*;

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct SerializableKeyValue {
        key: String,
        value: Option<String>,
    }
    
    impl From<KeyValue> for SerializableKeyValue {
        fn from(kv: KeyValue) -> Self {
            SerializableKeyValue {
                key: kv.key,
                value: kv.value,
            }
        }
    }
    impl From<SerializableKeyValue> for KeyValue {
        fn from(kv: SerializableKeyValue) -> Self {
            KeyValue {
                key: kv.key,
                value: kv.value,
            }
        }
    }

    #[allow(unused)]
    pub fn serialize<S>(data: &Option<Vec<KeyValue>>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let data: Option<Vec<SerializableKeyValue>> = data
            .as_ref()
            .map(|data| data.iter().map(|kv| kv.clone().into()).collect());
        match data {
            Some(data) => data.serialize(serializer),
            None => serializer.serialize_none(),
        }
    }
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Vec<KeyValue>>, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let data: Option<Vec<SerializableKeyValue>> = Option::deserialize(deserializer)?;
        let data = data.map(|data| data.into_iter().map(|kv| kv.into()).collect());
        Ok(data)
    }
}

/// Metadata for a Parquet file.
// This is almost equal to [`parquet_format_safe::FileMetaData`] but contains the descriptors,
// which are crucial to deserialize pages.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde_types", derive(Deserialize, Serialize))]
pub struct FileMetaData {
    /// version of this file.
    pub version: i32,
    /// number of rows in the file.
    pub num_rows: usize,
    /// String message for application that wrote this file.
    ///
    /// This should have the following format:
    /// `<application> version <application version> (build <application build hash>)`.
    ///
    /// ```shell
    /// parquet-mr version 1.8.0 (build 0fda28af84b9746396014ad6a415b90592a98b3b)
    /// ```
    pub created_by: Option<String>,
    /// The row groups of this file
    pub row_groups: Vec<RowGroupMetaData>,
    /// key_value_metadata of this file.
    #[cfg_attr(feature = "serde_types", serde(with = "key_value_serde"))]
    pub key_value_metadata: Option<Vec<KeyValue>>,
    /// schema descriptor.
    pub schema_descr: SchemaDescriptor,
    /// Column (sort) order used for `min` and `max` values of each column in this file.
    ///
    /// Each column order corresponds to one column, determined by its position in the
    /// list, matching the position of the column in the schema.
    ///
    /// When `None` is returned, there are no column orders available, and each column
    /// should be assumed to have undefined (legacy) column order.
    pub column_orders: Option<Vec<ColumnOrder>>,
}

impl FileMetaData {
    /// Returns the [`SchemaDescriptor`] that describes schema of this file.
    pub fn schema(&self) -> &SchemaDescriptor {
        &self.schema_descr
    }

    /// returns the metadata
    pub fn key_value_metadata(&self) -> &Option<Vec<KeyValue>> {
        &self.key_value_metadata
    }

    /// Returns column order for `i`th column in this file.
    /// If column orders are not available, returns undefined (legacy) column order.
    pub fn column_order(&self, i: usize) -> ColumnOrder {
        self.column_orders
            .as_ref()
            .map(|data| data[i])
            .unwrap_or(ColumnOrder::Undefined)
    }

    /// Deserializes [`crate::thrift_format::FileMetaData`] into this struct
    pub fn try_from_thrift(metadata: parquet_format_safe::FileMetaData) -> Result<Self, Error> {
        let schema_descr = SchemaDescriptor::try_from_thrift(&metadata.schema)?;

        let row_groups = metadata
            .row_groups
            .into_iter()
            .map(|rg| RowGroupMetaData::try_from_thrift(&schema_descr, rg))
            .collect::<Result<_, Error>>()?;

        let column_orders = metadata
            .column_orders
            .map(|orders| parse_column_orders(&orders, &schema_descr));

        Ok(FileMetaData {
            version: metadata.version,
            num_rows: metadata.num_rows.try_into()?,
            created_by: metadata.created_by,
            row_groups,
            key_value_metadata: metadata.key_value_metadata,
            schema_descr,
            column_orders,
        })
    }

    /// Serializes itself to thrift's [`parquet_format_safe::FileMetaData`].
    pub fn into_thrift(self) -> parquet_format_safe::FileMetaData {
        parquet_format_safe::FileMetaData {
            version: self.version,
            schema: self.schema_descr.into_thrift(),
            num_rows: self.num_rows as i64,
            row_groups: self
                .row_groups
                .into_iter()
                .map(|v| v.into_thrift())
                .collect(),
            key_value_metadata: self.key_value_metadata,
            created_by: self.created_by,
            column_orders: None, // todo
            encryption_algorithm: None,
            footer_signing_key_metadata: None,
        }
    }
}

/// Parses [`ColumnOrder`] from Thrift definition.
fn parse_column_orders(
    orders: &[TColumnOrder],
    schema_descr: &SchemaDescriptor,
) -> Vec<ColumnOrder> {
    schema_descr
        .columns()
        .iter()
        .zip(orders.iter())
        .map(|(column, order)| match order {
            TColumnOrder::TYPEORDER(_) => {
                let sort_order = get_sort_order(
                    &column.descriptor.primitive_type.logical_type,
                    &column.descriptor.primitive_type.converted_type,
                    &column.descriptor.primitive_type.physical_type,
                );
                ColumnOrder::TypeDefinedOrder(sort_order)
            }
        })
        .collect()
}
