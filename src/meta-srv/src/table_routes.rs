// Copyright 2023 Greptime Team
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use api::v1::meta::TableRouteValue;
use common_meta::helper::{TableGlobalKey, TableGlobalValue};
use common_meta::key::table_info::TableInfoValue;
use common_meta::key::TableRouteKey;
use common_meta::rpc::store::PutRequest;
use common_meta::table_name::TableName;
use snafu::{OptionExt, ResultExt};
use table::engine::TableReference;

use crate::error::{
    DecodeTableRouteSnafu, InvalidCatalogValueSnafu, Result, TableMetadataManagerSnafu,
    TableRouteNotFoundSnafu,
};
use crate::metasrv::Context;
use crate::service::store::kv::KvStoreRef;

pub async fn get_table_global_value(
    kv_store: &KvStoreRef,
    key: &TableGlobalKey,
) -> Result<Option<TableGlobalValue>> {
    let kv = kv_store.get(&key.to_raw_key()).await?;
    kv.map(|kv| TableGlobalValue::from_bytes(kv.value).context(InvalidCatalogValueSnafu))
        .transpose()
}

pub(crate) async fn get_table_route_value(
    kv_store: &KvStoreRef,
    key: &TableRouteKey<'_>,
) -> Result<TableRouteValue> {
    let kv = kv_store
        .get(key.to_string().as_bytes())
        .await?
        .with_context(|| TableRouteNotFoundSnafu {
            key: key.to_string(),
        })?;
    kv.value().try_into().context(DecodeTableRouteSnafu)
}

pub(crate) async fn put_table_route_value(
    kv_store: &KvStoreRef,
    key: &TableRouteKey<'_>,
    value: TableRouteValue,
) -> Result<()> {
    let req = PutRequest {
        key: key.to_string().into_bytes(),
        value: value.into(),
        prev_kv: false,
    };
    let _ = kv_store.put(req).await?;
    Ok(())
}

pub(crate) fn table_route_key(table_id: u32, t: &TableGlobalKey) -> TableRouteKey<'_> {
    TableRouteKey {
        table_id,
        catalog_name: &t.catalog_name,
        schema_name: &t.schema_name,
        table_name: &t.table_name,
    }
}

pub(crate) async fn fetch_table(
    kv_store: &KvStoreRef,
    table_ref: TableReference<'_>,
) -> Result<Option<(TableGlobalValue, TableRouteValue)>> {
    let tgk = TableGlobalKey {
        catalog_name: table_ref.catalog.to_string(),
        schema_name: table_ref.schema.to_string(),
        table_name: table_ref.table.to_string(),
    };

    let tgv = get_table_global_value(kv_store, &tgk).await?;

    if let Some(tgv) = tgv {
        let trk = table_route_key(tgv.table_id(), &tgk);
        let trv = get_table_route_value(kv_store, &trk).await?;

        return Ok(Some((tgv, trv)));
    }

    Ok(None)
}

pub(crate) async fn fetch_tables(
    ctx: &Context,
    table_names: Vec<TableName>,
) -> Result<Vec<(TableInfoValue, TableRouteValue)>> {
    let kv_store = &ctx.kv_store;

    let mut tables = vec![];
    // Maybe we can optimize the for loop in the future, but in general,
    // there won't be many keys, in fact, there is usually just one.
    for table_name in table_names {
        let Some(tgv) = ctx.table_metadata_manager
            .table_info_manager()
            .get_old(&table_name)
            .await
            .context(TableMetadataManagerSnafu)? else {
            continue;
        };
        let table_info = &tgv.table_info;

        let trk = TableRouteKey {
            table_id: table_info.ident.table_id,
            catalog_name: &table_info.catalog_name,
            schema_name: &table_info.schema_name,
            table_name: &table_info.name,
        };
        let trv = get_table_route_value(kv_store, &trk).await?;

        tables.push((tgv, trv));
    }

    Ok(tables)
}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use api::v1::meta::{Peer, Region, RegionRoute, Table, TableRoute};
    use chrono::DateTime;
    use common_catalog::consts::{DEFAULT_CATALOG_NAME, DEFAULT_SCHEMA_NAME, MITO_ENGINE};
    use common_meta::key::table_region::RegionDistribution;
    use common_meta::key::TableMetadataManagerRef;
    use datatypes::data_type::ConcreteDataType;
    use datatypes::schema::{ColumnSchema, RawSchema};
    use table::metadata::{RawTableInfo, RawTableMeta, TableIdent, TableType};
    use table::requests::TableOptions;

    use super::*;
    use crate::error;
    use crate::service::store::memory::MemStore;

    pub(crate) async fn prepare_table_region_and_info_value(
        table_metadata_manager: &TableMetadataManagerRef,
        table: &str,
    ) {
        let table_info = RawTableInfo {
            ident: TableIdent::new(1),
            name: table.to_string(),
            desc: None,
            catalog_name: DEFAULT_CATALOG_NAME.to_string(),
            schema_name: DEFAULT_SCHEMA_NAME.to_string(),
            meta: RawTableMeta {
                schema: RawSchema::new(vec![ColumnSchema::new(
                    "a",
                    ConcreteDataType::string_datatype(),
                    true,
                )]),
                primary_key_indices: vec![],
                value_indices: vec![],
                engine: MITO_ENGINE.to_string(),
                next_column_id: 1,
                region_numbers: vec![1, 2, 3, 4],
                engine_options: HashMap::new(),
                options: TableOptions::default(),
                created_on: DateTime::default(),
            },
            table_type: TableType::Base,
        };
        table_metadata_manager
            .table_info_manager()
            .put_old(table_info)
            .await
            .unwrap();

        // Region distribution:
        // Datanode => Regions
        // 1 => 1, 2
        // 2 => 3
        // 3 => 4
        table_metadata_manager
            .table_region_manager()
            .put_old(
                &TableName::new(DEFAULT_CATALOG_NAME, DEFAULT_SCHEMA_NAME, table),
                RegionDistribution::from([(1, vec![1, 2]), (2, vec![3]), (3, vec![4])]),
            )
            .await
            .unwrap();
    }

    pub(crate) async fn prepare_table_route_value<'a>(
        kv_store: &'a KvStoreRef,
        table: &'a str,
    ) -> (TableRouteKey<'a>, TableRouteValue) {
        let key = TableRouteKey {
            table_id: 1,
            catalog_name: DEFAULT_CATALOG_NAME,
            schema_name: DEFAULT_SCHEMA_NAME,
            table_name: table,
        };

        let peers = (1..=3)
            .map(|id| Peer {
                id,
                addr: "".to_string(),
            })
            .collect::<Vec<_>>();

        // region routes:
        // region number => leader node
        // 1 => 1
        // 2 => 1
        // 3 => 2
        // 4 => 3
        let region_routes = vec![
            new_region_route(1, &peers, 1),
            new_region_route(2, &peers, 1),
            new_region_route(3, &peers, 2),
            new_region_route(4, &peers, 3),
        ];
        let table_route = TableRoute {
            table: Some(Table {
                id: 1,
                table_name: Some(
                    TableName::new(DEFAULT_CATALOG_NAME, DEFAULT_SCHEMA_NAME, table).into(),
                ),
                table_schema: vec![],
            }),
            region_routes,
        };
        let value = TableRouteValue {
            peers,
            table_route: Some(table_route),
        };
        put_table_route_value(kv_store, &key, value.clone())
            .await
            .unwrap();
        (key, value)
    }

    pub(crate) fn new_region_route(
        region_number: u64,
        peers: &[Peer],
        leader_node: u64,
    ) -> RegionRoute {
        let region = Region {
            id: region_number,
            name: "".to_string(),
            partition: None,
            attrs: HashMap::new(),
        };
        let leader_peer_index = peers
            .iter()
            .enumerate()
            .find_map(|(i, peer)| {
                if peer.id == leader_node {
                    Some(i as u64)
                } else {
                    None
                }
            })
            .unwrap();
        RegionRoute {
            region: Some(region),
            leader_peer_index,
            follower_peer_indexes: vec![],
        }
    }

    #[tokio::test]
    async fn test_put_and_get_table_route_value() {
        let kv_store = Arc::new(MemStore::new()) as _;

        let key = TableRouteKey {
            table_id: 1,
            catalog_name: "not_exist_catalog",
            schema_name: "not_exist_schema",
            table_name: "not_exist_table",
        };
        assert!(matches!(
            get_table_route_value(&kv_store, &key).await.unwrap_err(),
            error::Error::TableRouteNotFound { .. }
        ));

        let (key, value) = prepare_table_route_value(&kv_store, "my_table").await;
        let actual = get_table_route_value(&kv_store, &key).await.unwrap();
        assert_eq!(actual, value);
    }
}
