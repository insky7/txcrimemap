use crate::{
    constants::{DYNAMO_TABLE_NAME, S3_BUCKET},
    modules::{helper_functions, Center, GeocodeResponse},
};
use aws_sdk_dynamodb::config::Region;
use aws_sdk_dynamodb::types::AttributeValue;
use aws_sdk_dynamodb::Client as DynamoClient;
use aws_sdk_s3::Client as S3Client;
use axum::{http::StatusCode, Form, Json};
use futures::future::join_all;
use geo::Geometry;
use geojson::GeoJson;
use google_maps::{Client, PlaceType};
use rust_decimal::prelude::*;
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use wkt::Wkt;

// get/init client for AWS S3
pub async fn get_client() -> aws_sdk_s3::Client {
    #[allow(deprecated)]
    // from env is apparently deprecated but still works so idgaf
    let config = aws_config::from_env()
        .region(aws_sdk_s3::config::Region::new("us-west-1"))
        .load()
        .await;
    aws_sdk_s3::Client::new(&config)
}

// download object from S3 bucket given key, bucket_name, and client (use get_client() for the client)
pub async fn download_object(
    client: &aws_sdk_s3::Client,
    bucket_name: &str,
    key: &str,
) -> Result<aws_sdk_s3::operation::get_object::GetObjectOutput, String> {
    client
        .get_object()
        .bucket(bucket_name)
        .key(key)
        .send()
        .await
        .map_err(|e| format!("error code: {}", e))
}

/*
query dynamo table with a list of counties
returns a vector of items (HashMap<String, AttributeValue>)
each item is a HashMap with string keys and AttributeValue values
this function is async and uses tokio to spawn tasks for each county
it uses join_all to wait for all tasks to complete and returns the results in a single vector to return to the caller
if any task fails, it returns an error
*/
pub async fn query_dynamo(
    client: &DynamoClient,
    table_name: &str,
    counties: Vec<String>,
) -> Result<Vec<HashMap<String, AttributeValue>>, aws_sdk_dynamodb::Error> {
    let mut tasks = Vec::new();

    for county in counties {
        // might be better to use a thread pool for this, but for now, we'll just spawn a task for each county
        // this is a simple example, so we'll just spawn a task for each county
        let client = client.clone(); // cloning client, probably not that expensive
        let table_name = table_name.to_string();

        // Must add county to the string to match the format in the database
        // Assuming the county names in the database are formatted as "CountyName County"
        let county_full = format!("{} County", county);

        let task = tokio::spawn(async move {
            let mut results = Vec::new();
            let mut last_evaluated_key: Option<HashMap<String, AttributeValue>> = None;

            loop {
                let mut request = client
                    .query()
                    .table_name(&table_name)
                    // indexed on county name
                    .index_name("CountyIndex")
                    .key_condition_expression("#county = :county")
                    .expression_attribute_names("#county", "County")
                    .expression_attribute_values(":county", AttributeValue::S(county_full.clone()));

                // this is the last evaluated key from the previous request
                if let Some(lek) = &last_evaluated_key {
                    request = request.set_exclusive_start_key(Some(lek.clone()));
                }

                let response = request.send().await?;

                if let Some(items) = response.items {
                    results.extend(items);
                }

                if let Some(lek) = response.last_evaluated_key {
                    last_evaluated_key = Some(lek);
                } else {
                    break;
                }
            }
            // if we have no results, return an empty vector
            if results.is_empty() {
                return Ok(results);
            }
            Ok::<_, aws_sdk_dynamodb::Error>(results)
        });

        tasks.push(task);
    }

    let mut all_results = Vec::new();

    for task in join_all(tasks).await {
        let items = task.unwrap()?; // unwrap tokio task, propagate query error if any https://youtu.be/w9dqoVy6szc
        all_results.extend(items);
    }

    Ok(all_results)
}

/*
load the neighbor map (texas_county_neighbors.json) from S3 bucket and return it as a HashMap<String, Vec<String>> for easy lookup
the HashMap is a map of counties to their neighbors, where
the key is the county name and the value is a vector of neighboring counties
*/
pub async fn load_neighbor_map_from_s3(
    client: &S3Client,
    bucket: &str,
    key: &str,
) -> Result<HashMap<String, Vec<String>>, Box<dyn std::error::Error>> {
    let output = download_object(client, bucket, key).await?;
    let body = output.body.collect().await?;
    let bytes = body.into_bytes();
    let map: HashMap<String, Vec<String>> = serde_json::from_slice(&bytes)?;
    Ok(map)
}

/*
geocode function to handle the form submission and return the geocoded response
this function is called when the form is submitted and returns a JSON response with the geocoded data
it uses the Google Maps API to geocode the address and returns the latitude and longitude of the center of the area
it also queries the DynamoDB table for the counties in the area and returns the crime data for those counties
*/
pub async fn geocode(
    Form(form_data): Form<super::MyForm>,
) -> Result<impl axum::response::IntoResponse, StatusCode> {
    let google_maps_client =
        Client::try_new(super::GOOGLE_API_KEY).expect("Failed to initialize Google Maps client");

    let geocode_res = google_maps_client
        .geocoding()
        .with_address(&form_data.address)
        .execute()
        .await
        .map_err(|err| {
            eprintln!("Geocoding error: {:?}", err);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let results = geocode_res.results;
    if let Some(first) = results.first() {
        let center_lat = first.geometry.location.lat.to_f64().unwrap();
        let center_lon = first.geometry.location.lng.to_f64().unwrap();
        println!("Geocoded center: ({}, {})", center_lat, center_lon);

        let mut target_county: Option<String> = None;
        for comp in &first.address_components {
            for t in &comp.types {
                if t == &PlaceType::AdministrativeAreaLevel2 {
                    target_county = Some(comp.long_name.replace(" County", ""));
                    break;
                }
            }
            if target_county.is_some() {
                break;
            }
        }
        let target_county = target_county.ok_or(StatusCode::NOT_FOUND)?;
        println!("Target county: {}", target_county);

        let s3 = helper_functions::get_client().await;

        // load the neighbor map from S3
        let neighbors_map = helper_functions::load_neighbor_map_from_s3(
            &s3,
            S3_BUCKET,
            "texas_county_neighbors.json",
        )
        .await
        .map_err(|e| {
            eprintln!("Failed to load neighbors map from S3: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        let mut counties_to_query = vec![target_county.clone()];
        if let Some(neighbors) = neighbors_map.get(&target_county) {
            counties_to_query.extend(neighbors.clone());
        }
        println!("Target counties: {}", counties_to_query.join(", "));

        #[allow(deprecated)]
        let config = aws_config::from_env()
            .region(Region::new("us-west-1"))
            .load()
            .await;
        let dynamo_client = DynamoClient::new(&config);

        // query the DynamoDB table for the counties from form data
        // this is the main part of the function where we query the DynamoDB table for the counties
        let items = query_dynamo(&dynamo_client, DYNAMO_TABLE_NAME, counties_to_query)
            .await
            .map_err(|e| {
                eprintln!("Failed to query DynamoDB: {:?}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?;

        let mut areas = Vec::new();

        for item in items {
            let geo_id = item
                .get("GEOID")
                .and_then(|v| v.as_s().ok())
                .unwrap()
                .to_string();

            let county_name = item
                .get("County")
                .and_then(|v| v.as_s().ok())
                .unwrap()
                .to_string();

            let wkt_geometry = item
                .get("Geometry")
                .and_then(|v| v.as_s().ok())
                .unwrap()
                .to_string();

            let crime_percentile = item
                .get("WeightedCrimePercentile")
                .and_then(|v| v.as_n().ok())
                .and_then(|n| n.parse::<f64>().ok())
                .unwrap_or(0.0);

            /*
            convert WKT to GeoJSON
            this is where we convert the WKT string to GeoJSON using the wkt crate
            and the geojson crate to create a GeoJSON object
            we use the wkt_to_geojson function to do this
            and check if the conversion was successful
            if it was, we push the GeoJSON object to the areas vector
            if it wasn't, we just skip this item
            and continue to the next item
            */

            if let Some(geojson) = wkt_to_geojson(&wkt_geometry) {
                areas.push(serde_json::json!({
                    "geo_id": geo_id,
                    "county": county_name,
                    "crime_percentile": crime_percentile,
                    "geometry": geojson
                }));
            }
        }

        // if we have no areas, return a 404 error
        if areas.is_empty() {
            return Err(StatusCode::NOT_FOUND);
        }

        // this is where we create the response object with the center and areas
        let response = GeocodeResponse {
            center: Center {
                lat: center_lat,
                lon: center_lon,
            },
            areas,
        };

        let json_response = Json(response);
        return Ok((StatusCode::OK, json_response));
    }
    Err(StatusCode::NOT_FOUND)
}

// helper fn to convert WKT to GeoJSON
fn wkt_to_geojson(wkt_str: &str) -> Option<JsonValue> {
    let wkt_parsed: Wkt<f64> = wkt_str.parse().ok()?;
    if let Some(item) = wkt_parsed.into() {
        let geo_geom: Geometry<f64> = item.try_into().ok()?;
        let geojson = GeoJson::from(&geo_geom);
        serde_json::to_value(geojson).ok()
    } else {
        None
    }
}
