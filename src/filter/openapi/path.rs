use wildmatch::WildMatch;
use serde_json::{Value,Map};
use std::collections::{HashMap,HashSet};

use crate::filter::reference_collector;
#[allow(dead_code)]
pub fn create_empty_value() -> Value {
    Value::Object(Map::new())
}

pub fn filter(openapi: &Value,names: &Vec<String>){
    let path_matchers: Vec<WildMatch> = names.iter().map(|name| WildMatch::new(name)).collect();

    let openapi_path = openapi.get("paths").unwrap();
    // Check if it's an object and collect keys
       if let Value::Object(map) = openapi_path {
           let path_names: Vec<String> = map.keys().cloned().collect();
           dbg!("Keys: {:?}", path_names);
           let filtered_keys: Vec<&String> = path_names.iter()
               .filter(|openapi_path|path_matchers.iter().any(|pattern| pattern.matches(openapi_path)))
               .collect();
           dbg!("Filtered Keys: {:?}", filtered_keys);
           let mut collected_references: HashSet<String> = HashSet::with_capacity(10);
           for path_name in filtered_keys {

                   if let Some(value) = openapi_path.get(path_name) {
                       reference_collector::collect_path_refs(value,&mut collected_references,None);
                   } else {
                       dbg!("Field '{}' not found.", path_name);
                   }
               }
               let mut target_document = create_empty_value();
               if let Value::Object(map) = &mut target_document {
                       clone_base_api_fields(openapi, map);
                       map.insert("paths".to_string(), openapi_path.clone());


                   }

               let mut component_references: HashMap<String,Vec<String>> = HashMap::new();
               reference_collector::collect_component_refs(openapi.get("components").unwrap(),"#/components",&mut component_references,2,0);
               let kept_references: HashMap<String,Vec<String>> = component_references.into_iter().filter(|(key,_)|collected_references.contains(key)).collect();
               dbg!("references: {:?}",collected_references);
               dbg!("kept: {:?}",kept_references);


       } else {
           dbg!("The JSON value is not an object.");
       }
}

fn clone_base_api_fields(openapi: &Value, map: &mut serde_json::Map<String, Value>) {
    match openapi.get("info"){
           Some(info) => {map.insert("info".to_string(),info.clone());}
           None => {}
       }

    match openapi.get("servers"){
           Some(servers) => {map.insert("servers".to_string(),servers.clone());}
           None => {}
       }

    match openapi.get("openapi"){
           Some(openapi) => {map.insert("openapi".to_string(),openapi.clone());}
           None => {}
       }
}
use wildmatch::WildMatch;
use serde_json::{Value,Map};
use std::collections::{HashMap,HashSet};

use crate::filter::reference_collector;
/// Creates an empty JSON Value object.
#[allow(dead_code)]
pub fn create_empty_value() -> Value {
    Value::Object(Map::new())
}

/// Filters the OpenAPI specification based on provided path names.
///
/// This function takes an OpenAPI `Value` and a vector of path names as input.
/// It filters the paths in the OpenAPI specification to include only those that match the provided names.
/// It also collects references to components used in the filtered paths.
///
/// # Arguments
///
/// * `openapi` - A reference to the OpenAPI specification as a `Value`.
/// * `names` - A reference to a vector of path names to filter by.
pub fn filter(openapi: &Value,names: &Vec<String>){
    let path_matchers: Vec<WildMatch> = names.iter().map(|name| WildMatch::new(name)).collect();

    let openapi_path = openapi.get("paths").unwrap();
    // Check if it's an object and collect keys
       if let Value::Object(map) = openapi_path {
           let path_names: Vec<String> = map.keys().cloned().collect();
           dbg!("Keys: {:?}", path_names);
           let filtered_keys: Vec<&String> = path_names.iter()
               .filter(|openapi_path|path_matchers.iter().any(|pattern| pattern.matches(openapi_path)))
               .collect();
           dbg!("Filtered Keys: {:?}", filtered_keys);
           let mut collected_references: HashSet<String> = HashSet::with_capacity(10);
           for path_name in filtered_keys {

                   if let Some(value) = openapi_path.get(path_name) {
                       reference_collector::collect_path_refs(value,&mut collected_references,None);
                   } else {
                       dbg!("Field '{}' not found.", path_name);
                   }
               }
               let mut target_document = create_empty_value();
               if let Value::Object(map) = &mut target_document {
                       clone_base_api_fields(openapi, map);
                       map.insert("paths".to_string(), openapi_path.clone());


                   }

               let mut component_references: HashMap<String,Vec<String>> = HashMap::new();
               reference_collector::collect_component_refs(openapi.get("components").unwrap(),"#/components",&mut component_references,2,0);
               let kept_references: HashMap<String,Vec<String>> = component_references.into_iter().filter(|(key,_)|collected_references.contains(key)).collect();
               dbg!("references: {:?}",collected_references);
               dbg!("kept: {:?}",kept_references);


       } else {
           dbg!("The JSON value is not an object.");
       }
}

/// Clones base API fields (info, servers, openapi) from the original OpenAPI specification to the target document.
///
/// This function copies the "info", "servers", and "openapi" fields from the original OpenAPI `Value`
/// to the provided target `Map`. This ensures that the filtered document retains essential metadata
/// from the original specification.
///
/// # Arguments
///
/// * `openapi` - A reference to the original OpenAPI specification as a `Value`.
/// * `map` - A mutable reference to the target `Map` where the base API fields will be inserted.
fn clone_base_api_fields(openapi: &Value, map: &mut serde_json::Map<String, Value>) {
    match openapi.get("info"){
           Some(info) => {map.insert("info".to_string(),info.clone());}
           None => {}
       }

    match openapi.get("servers"){
           Some(servers) => {map.insert("servers".to_string(),servers.clone());}
           None => {}
       }

    match openapi.get("openapi"){
           Some(openapi) => {map.insert("openapi".to_string(),openapi.clone());}
           None => {}
       }
}
