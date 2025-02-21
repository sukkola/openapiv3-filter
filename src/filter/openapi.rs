use openapiv3::{OpenAPI, Paths, PathItem, ReferenceOr,Operation,Components,Tag,SecurityScheme};
use indexmap::map::IndexMap;
use wildmatch::WildMatch;
use std::collections::{HashMap,HashSet};
use serde_json::json;

// Define the filtering trait
use crate::filter::content::reference_collector::{collect_path_refs, collect_operation_tags,collect_operation_securities};
use crate::filter::content::reference_processor;
use crate::filter::content::reference_collector;
use crate::filter::content::json_path_filter;

///Type that is used for filtering openapi paths
type PathFilter<'d> = Box<dyn Fn(&(&String, &ReferenceOr<PathItem>)) ->  bool + 'd>;
///Type that is used for filtering openapi operations
type OperationFilter<'d> = Box<dyn Fn(&(&str, &Operation)) ->  bool + 'd>;

///Filtering parameters for the filtering trait
#[derive(Debug, Default)]
pub struct FilteringParameters{
    ///when provided only outputs paths that match the parameters
    pub paths: Option<Vec<String>>,
    ///when provided only outputs tags that match the parameters
    pub tags: Option<Vec<String>>,
    ///when provided only outputs http methods that match the parameters
    pub methods: Option<Vec<String>>,
    ///when provided only outputs endpoints that use provided security parameters
    pub security: Option<Vec<String>>,
    //pub content_types: Option<&'a Vec<String>>

}

///Adds filtering capability to OpenAPI
pub trait OpenAPIFilter {
   /// Filters an OpenAPI document based on provided criteria
   ///
   /// This trait provides a method to filter and extract portions of an OpenAPI document according to specified parameters.
   /// The filtering can be done by paths, tags, HTTP methods, security schemes, and other criteria while maintaining referential integrity
   /// for used components and definitions.
    fn filter_by_parameters(&self, filters: FilteringParameters) -> Option<Self>
    where
        Self: Sized;
}

/// Filtering implementation for OpenAPI documents
///
/// This implementation provides methods to filter and extract portions of an OpenAPI document according to specified parameters.
/// The filtering can be done by paths, tags, HTTP methods, security schemes, and other criteria while maintaining referential integrity
/// for used components and definitions.
impl OpenAPIFilter for OpenAPI{

    ///Returns the partial openapi where non filtered items are removed from the api contents
    fn filter_by_parameters<'d>(&self, filters: FilteringParameters) -> Option<Self>
    where
        Self: Sized{
            let path_filters = map_path_name_filters(filters.paths);
            let path_tag_filters = map_path_tags_filters(filters.tags.clone());
            let path_security_filters = map_path_security_filters(filters.security.clone());

            let path_filters: Vec<PathFilter> =
                vec![path_filters,path_tag_filters,path_security_filters].into_iter().flatten().collect();

            let mut filtered_paths: IndexMap<String, ReferenceOr<PathItem>> = self
                        .paths
                        .iter()
                        .filter(|x| path_filters.iter().all(|filter| filter(x)))
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect();

            let operation_tag_filters =map_operation_tags_filters(filters.tags.clone());
            let allowed_tags: HashSet<String> = filters.tags.map_or_else(HashSet::new, |v| v.into_iter().collect());
            let operation_method_filters = map_operation_method_filters(filters.methods);
            let operation_security_filters = map_operation_security_filters(filters.security.clone());
            let allowed_securities: HashSet<String> = filters.security.map_or_else(HashSet::new, |v| v.into_iter().collect());

            let operation_filters: Vec<OperationFilter<'d>> =
                vec![operation_tag_filters,operation_method_filters,operation_security_filters].into_iter().flatten().collect();

            let mut components: HashSet<String> = HashSet::with_capacity(10);
            let mut tags: HashSet<String> = HashSet::with_capacity(10);
            let mut securities: HashSet<String> = HashSet::with_capacity(10);
            for (_, path_ref) in filtered_paths.iter_mut() {
                if let Some(old_path) = path_ref.as_item() {
                    let filtered_operations: HashMap<&str, &Operation> = old_path.iter()
                        .filter(|operation| operation_filters.iter().all(|filter| filter(operation)))
                        .collect();
                    collect_operation_tags(filtered_operations.values().collect(),&mut tags,&allowed_tags);
                    collect_operation_securities(filtered_operations.values().collect(),&mut securities,&allowed_securities);
                    // Create new PathItem
                    let new_path = PathItem {
                        get: filtered_operations.get("get").map(|op| clone_operation(op,&allowed_tags,&allowed_securities)),
                        put: filtered_operations.get("put").map(|op| clone_operation(op,&allowed_tags,&allowed_securities)),
                        post: filtered_operations.get("post").map(|op| clone_operation(op,&allowed_tags,&allowed_securities)),
                        delete: filtered_operations.get("delete").map(|op| clone_operation(op,&allowed_tags,&allowed_securities)),
                        options: filtered_operations.get("options").map(|op| clone_operation(op,&allowed_tags,&allowed_securities)),
                        head: filtered_operations.get("head").map(|op| clone_operation(op,&allowed_tags,&allowed_securities)),
                        patch: filtered_operations.get("patch").map(|op| clone_operation(op,&allowed_tags,&allowed_securities)),
                        trace: filtered_operations.get("trace").map(|op| clone_operation(op,&allowed_tags,&allowed_securities)),
                        ..old_path.clone()
                    };

                    collect_path_refs(&serde_json::to_value(&new_path).unwrap(),&mut components,None);
                    //collect_tags(&new_path,&mut tags,&allowed_tags);



                    // Assign the new path back to the container
                    *path_ref = ReferenceOr::Item(new_path);  // Adjust this line based on your actual container type
                }
            }
            let mut components_elements = found_refs_to_components(self,&mut components);
            let tags_elements = found_refs_to_tags(self,&tags);

            let paths_with_content: IndexMap<String, ReferenceOr<PathItem>> =
                filtered_paths.into_iter()
                              .filter(|(_,value)|value.as_item().is_some() &&
                                      value.as_item().unwrap().iter().count() > 0)
                              .collect();

            let default_map = IndexMap::<String, ReferenceOr<SecurityScheme>>::default();
            let security_schemes = self.components.as_ref()
                .map_or(&default_map, |c| &c.security_schemes);
            let filtered_securities = filter_securities(&securities, security_schemes);
            components_elements.security_schemes = filtered_securities;


            Some(OpenAPI {
                            paths: Paths {paths:paths_with_content, extensions: self.paths.extensions.clone()},
                            components: Some(components_elements),
                            tags: tags_elements,
                            ..self.clone()
                        })

        }
}

/// Filters out security schemes from the openapi document that are not present in operations after filtering them
///
/// # Arguments
///
/// * `allowed_securities` - names of the security definitions found from filtered paths.
/// * `security_schemes` - security scheme definitions in the openapi document.
///
/// # Returns
///
/// * `IndexMap<String, ReferenceOr<SecurityScheme>>` - An IndexMap of `SecurityScheme` values.
fn filter_securities(allowed_securities: &HashSet<String>, security_schemes: &IndexMap<String, ReferenceOr<SecurityScheme>>) -> IndexMap<String, ReferenceOr<SecurityScheme>> {

    let mut filtered_securities: IndexMap<String, ReferenceOr<SecurityScheme>> = IndexMap::new();
    security_schemes.iter().filter(|scheme|{
         allowed_securities.contains(scheme.0)
        }).for_each(|(key,value)|{
            filtered_securities.insert(key.clone(), value.clone());
        });
        filtered_securities
}

fn clone_operation(operation:&Operation,allowed_tags: &HashSet<String>,allowed_securities: &HashSet<String>) ->Operation{
    let filter_tags = allowed_tags.iter().count() > 0;
    let filter_securities = allowed_securities.iter().count() > 0;
    if filter_tags || filter_securities{
        let new_tags: Vec<String> = operation.tags.clone().into_iter().filter(|tag|allowed_tags.contains(tag)).collect();
         let mut new_security: Vec<IndexMap<String, Vec<String>>> = Vec::new();
        operation.security
            .iter()
            .for_each(|security_vec| {
                security_vec.iter().for_each(|old_map|{
                    let mut new_map: IndexMap<String, Vec<String>> = IndexMap::new();
                    old_map.iter()
                        .filter(|sec_map_item|allowed_securities.contains(sec_map_item.0))
                        .for_each(|(key,value)|{
                            new_map.insert(key.clone(), value.clone());
                        });
                    if new_map.iter().count() >0 {
                        new_security.insert(0, new_map);
                    }
                });


            });
        new_security.reverse();
        Operation{
            tags: new_tags,
            security :if !new_security.is_empty() {Some(new_security)}else{None},
            ..operation.clone()
        }
    }else{
        operation.clone()
    }

}


    fn map_path_name_filters<'d>(paths: Option<Vec<String>>) -> Vec<PathFilter<'d>> {
        let path_filters: Vec<PathFilter<'d>> =
            paths
                .into_iter()
                .map(|path_patterns| {
                    Box::new(move |(key, _value): &(&String, &ReferenceOr<PathItem>)| {
                        let path_matchers: Vec<WildMatch> = path_patterns.iter().map(|name| WildMatch::new(name)).collect();
                        path_matchers.iter().any(|pattern| pattern.matches(key.to_owned()))
                    }) as PathFilter<'d>
                })
                .collect();
        path_filters
    }

   /// Creates a vector of path filters based on provided tags
   ///
   /// This function converts an optional list of tags into filter closures that can be applied to OpenAPI paths.
   /// The filters check if any operation in the path has a matching tag.
   /// Runs the filtering on all operations under path to select paths to keep in document
   ///
   /// # Arguments
   /// * `tags` - An optional list of tag names
   ///
   /// # Returns
   /// A vector of filter closures that can be applied to OpenAPI paths
    fn map_path_tags_filters<'d>(tags: Option<Vec<String>>) -> Vec<PathFilter<'d>> {
        let path_filters: Vec<PathFilter<'d>> =
            tags
                .into_iter()
                .map(|tags| {
                    Box::new(move |(_key, reference_or_path): &(&String, &ReferenceOr<PathItem>)| {
                        reference_or_path.to_owned().as_item().unwrap().iter()
                            .any(|(_str,operation)|operation.tags.iter()
                                .any(|tag|tags.contains(tag)))
                    }) as PathFilter<'d>
                })
                .collect();
        path_filters
    }

    /// Creates a vector of path filters based on security requirements
    ///
    /// This function converts an optional list of security schemes into filter closures that can be applied to OpenAPI paths.
    /// The filters check if any operation in the path uses one of the specified security schemes.
    /// Runs filtering for all the methods under path to find out which paths to keep
    ///
    /// # Arguments
    /// * `securities` - An optional list of security scheme names
    ///
    /// # Returns
    /// A vector of filter closures that can be applied to OpenAPI paths
    ///
    fn map_path_security_filters<'d>(securities: Option<Vec<String>>) -> Vec<PathFilter<'d>> {
        let path_filters: Vec<PathFilter<'d>> =
            securities
                .into_iter()
                .map(|securities| {
                    Box::new(move |(_key, reference_or_path): &(&String, &ReferenceOr<PathItem>)| {
                        reference_or_path.to_owned().as_item().unwrap().iter()
                            .any(|(_str,operation)|operation.security.iter()
                                .any(|security|security.iter().any(|item|item.keys().any(|security_name|securities.contains(security_name)))))
                    }) as PathFilter<'d>
                })
                .collect();
        path_filters
    }

    /// Creates a vector of path filters based on provided tags
    ///
    /// This function converts an optional list of tags into filter closures that can be applied to OpenAPI operations.
    /// The filters check if any operation in the operation has a matching tag.
    ///
    /// # Arguments
    /// * `tags` - An optional list of tag names
    ///
    /// # Returns
    /// A vector of filter closures that can be applied to OpenAPI paths
    fn map_operation_tags_filters<'d>(tags: Option<Vec<String>>) -> Vec<OperationFilter<'d>> {
        let operation_filters: Vec<OperationFilter<'d>> =
            tags
                .into_iter()
                .map(|operations| {
                    Box::new(move |(_key, operation): &(&str, &Operation)| {
                        operation.tags.iter()
                            .any(|tag|operations.contains(tag))
                    }) as OperationFilter<'d>
                })
                .collect();
        operation_filters
    }

    /// Creates a vector of method filters based on requirements
    ///
    /// This function converts an optional list of operations into filter closures that can be applied to OpenAPI operations.
    /// The filters check if any operation in the path uses one of the specified security schemes.
    ///
    /// # Arguments
    /// * `methods` - An optional list of http methods
    ///
    /// # Returns
    /// A vector of filter closures that can be applied to OpenAPI paths
    ///
    fn map_operation_method_filters<'d>(operations: Option<Vec<String>>) -> Vec<OperationFilter<'d>> {
        let operation_filters: Vec<OperationFilter<'d>> =
            operations
                .into_iter()
                .map(|operations| {
                    Box::new(move |(operation_name, _operation): &(&str, &Operation)| {
                        operations.contains(&operation_name.to_string())
                    }) as OperationFilter<'d>
                })
                .collect();
        operation_filters
    }

    /// Creates a vector of operation filters based on a list of security requirements.
    ///
    /// This function takes an optional list of security requirements and converts them into a vector of `OperationFilter` closures.
    /// Each `OperationFilter` closure checks if a given operation has any of the provided security requirements.
    ///
    /// # Arguments
    ///
    /// * `securitites` - An `Option` containing a vector of security requirements.
    ///
    /// # Returns
    ///
    /// * `Vec<OperationFilter<'d>>` - A vector of `OperationFilter` closures.
    fn map_operation_security_filters<'d>(securities: Option<Vec<String>>) -> Vec<OperationFilter<'d>> {
        let operation_filters: Vec<OperationFilter<'d>> = securities
            .into_iter()
            .map(|securities| {
                Box::new(move |(_key, operation): &(&str, &Operation)| {
                    operation.security.iter().any(|security| {
                        security
                            .iter()
                            .any(|map| map.keys().any(|key| securities.contains(key)))
                    })
                }) as OperationFilter<'d>
            })
            .collect();
        operation_filters
    }

   /// Filters and retains only used component references
   ///
   /// This function examines an OpenAPI document and its collection of referenced components, filtering out any components that are not actually referenced in the filtered paths.
   /// It ensures that only the necessary components remain in the document after filtering.
   ///
   /// # Arguments
   /// * `openapi` - The OpenAPI document to filter
   /// * `components` - A set of component names that have been referenced in the filtered paths
   ///
   /// # Returns
   /// The filtered Components object containing only used components
    fn found_refs_to_components(openapi: &OpenAPI,components: &mut HashSet<String>) -> Components {

        let mut component_references: HashMap<String,Vec<String>> = HashMap::new();

        reference_collector::collect_component_refs(serde_json::to_value(openapi).unwrap().get("components").unwrap(),"#/components",&mut component_references,2,0);
        let found_references: HashMap<String,Vec<String>> = component_references.into_iter().filter(|(key,_)|components.contains(key)).collect();
        let final_references = reference_processor::get_kept_references(&found_references,components);
        let component_json_paths: Vec<String> = final_references.iter()
            .filter(|component| component.starts_with("#/components/")).map(|component| &component[13..])
            .map(|component| component.split("/"))
            .map(|component_path_elements|  component_path_elements.collect::<Vec<_>>().join(".").to_owned()).collect();
        let component_json_path_refs: Vec<&str> = component_json_paths.iter().map(|path| path.as_str()).collect();

        let filtered_components = json_path_filter::filter_json(&serde_json::to_value(&openapi.components).unwrap(), &component_json_path_refs);
        match filtered_components {
            Some(filtered_components) => { serde_json::from_value(filtered_components).ok().unwrap() },
            None => { serde_json::from_value(json!({})).ok().unwrap() }
        }
    }

   /// Filters and retains only used tags
   ///
   /// This function examines an OpenAPI document and its collection of tags, filtering out any tags that are not actually referenced in the filtered paths.
   /// It ensures that only the necessary tags remain in the document after filtering.
   ///
   /// # Arguments
   /// * `openapi` - The OpenAPI document to filter
   /// * `tags` - A set of tag names that have been referenced in the filtered paths
   ///
   /// # Returns
   /// The filtered list of Tag objects containing only used tags
    fn found_refs_to_tags(openapi: &OpenAPI,tags: &HashSet<String>) -> Vec<Tag> {
       // dbg!("tags:{:?} found tags:{:?}",openapi.tags.clone(),tags);
        openapi.tags.iter().filter(|tag|tags.contains(&tag.name)).map(|tag|tag.to_owned()).collect()
    }

    #[cfg(test)]
    mod tests {
        use crate::parser;
        use insta::assert_json_snapshot;
        use super::*;
        use parser::ParsedType;

        #[test]
        fn it_filters_paths_with_no_matches() {
            let openapi: Result<ParsedType<OpenAPI>,Box<dyn (std::error::Error)>> = parser::parse_document(&String::from("tests/resources/user-reference.yaml"));
            let filtered_api = extract_content(openapi.unwrap()).filter_by_parameters(FilteringParameters{paths:Some(vec![String::from("non-matching-path")]),..Default::default()});
            assert!(filtered_api.is_some());
            assert_json_snapshot!(filtered_api);
        }

        #[test]
        fn it_filters_paths_with_partial_path_name_match() {
            let openapi: Result<ParsedType<OpenAPI>,Box<dyn (std::error::Error)>> = parser::parse_document(&String::from("tests/resources/user-reference.yaml"));
            let filtered_api = extract_content(openapi.unwrap()).filter_by_parameters(FilteringParameters{paths:Some(vec![String::from("*userId*")]),..Default::default()});
            assert!(filtered_api.is_some());
            assert_json_snapshot!(filtered_api);
        }

        #[test]
        fn it_filters_paths_with_method_name_match() {
            let openapi: Result<ParsedType<OpenAPI>,Box<dyn (std::error::Error)>> = parser::parse_document(&String::from("tests/resources/user-reference.yaml"));
            let filtered_api = extract_content(openapi.unwrap()).filter_by_parameters(FilteringParameters{methods:Some(vec![String::from("post")]),..Default::default()});
            assert!(filtered_api.is_some());
            assert_json_snapshot!(filtered_api);
        }

        #[test]
        fn it_filters_paths_with_tag_name_match() {
            let openapi: Result<ParsedType<OpenAPI>,Box<dyn (std::error::Error)>> = parser::parse_document(&String::from("tests/resources/user-reference.yaml"));
            let filtered_api = extract_content(openapi.unwrap()).filter_by_parameters(FilteringParameters{tags:Some(vec![String::from("item")]),..Default::default()});
            assert!(filtered_api.is_some());
            assert_json_snapshot!(filtered_api);
        }

        #[test]
        fn it_filters_paths_with_partial_path_tag_name_and_method_name_match() {
            let openapi: Result<ParsedType<OpenAPI>,Box<dyn (std::error::Error)>> = parser::parse_document(&String::from("tests/resources/user-reference.yaml"));
            let filtered_api = extract_content(openapi.unwrap()).filter_by_parameters(FilteringParameters{methods:Some(vec![String::from("get")]),tags:Some(vec![String::from("item")]),paths:Some(vec![String::from("*userId*")]),..Default::default()});
            assert!(filtered_api.is_some());
            assert_json_snapshot!(filtered_api);
        }

        #[test]
        fn it_filters_petstore_with_full_path() {
            let openapi: Result<ParsedType<OpenAPI>,Box<dyn (std::error::Error)>> = parser::parse_document(&String::from("tests/resources/petstore.yaml"));
            let filtered_api = extract_content(openapi.unwrap()).filter_by_parameters(FilteringParameters{paths:Some(vec![String::from("/pet/{petId}")]),methods:Some(vec![String::from("get")]),..Default::default()});
            assert!(filtered_api.is_some());
            assert_json_snapshot!(filtered_api);
        }

        #[test]
        fn it_filters_petstore_with_full_path_an_api_key_auth() {
            let openapi: Result<ParsedType<OpenAPI>,Box<dyn (std::error::Error)>> = parser::parse_document(&String::from("tests/resources/petstore.yaml"));
            let filtered_api = extract_content(openapi.unwrap()).filter_by_parameters(FilteringParameters{paths:Some(vec![String::from("/pet/{petId}")]),methods:Some(vec![String::from("get")]),security:Some(vec![String::from("api_key")]),..Default::default()});
            assert!(filtered_api.is_some());
            assert_json_snapshot!(filtered_api);
        }

        #[test]
        fn it_filters_petstore_with_partial_path_and_does_not_keep_unnecessary_security_schemes() {
            let openapi: Result<ParsedType<OpenAPI>,Box<dyn (std::error::Error)>> = parser::parse_document(&String::from("tests/resources/petstore.yaml"));
            let filtered_api = extract_content(openapi.unwrap()).filter_by_parameters(FilteringParameters{paths:Some(vec![String::from("*createWithList")]),..Default::default()});
            assert!(filtered_api.is_some());
            assert_json_snapshot!(filtered_api);
        }

        fn extract_content<T>(parsed: ParsedType<T>) -> T {
            match parsed {
                ParsedType::JSON(content) => content,
                ParsedType::YAML(content) => content,
            }
        }
    }
