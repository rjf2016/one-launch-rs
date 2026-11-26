use std::sync::{Mutex, PoisonError};

use ol_platform_api::http::{HttpClient, HttpError, HttpRequest, HttpResponse, TooLarge};

type Responder = dyn Fn(&HttpRequest) -> Result<HttpResponse, HttpError> + Send + Sync;

// One request as it was asked. The key itself is not kept: only that the request carried it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asked {
    pub url: String,
    pub with_key: bool,
    // Every header the request would send beside Host, by name.
    pub headers: Vec<&'static str>,
}

// Stands where the internet would: every request is written down and answered in this process, by the responder it was made with. It holds an answer to what the real client promises: a body for a 2xx answer alone, and never one past the request's limit.
pub struct RecordingHttp {
    respond: Box<Responder>,
    asked: Mutex<Vec<Asked>>,
}

impl RecordingHttp {
    pub fn new(
        respond: impl Fn(&HttpRequest) -> Result<HttpResponse, HttpError> + Send + Sync + 'static,
    ) -> Self {
        Self {
            respond: Box::new(respond),
            asked: Mutex::default(),
        }
    }

    // Answers every request with 404.
    pub fn answering_nothing() -> Self {
        Self::new(|_| Ok(status(404)))
    }

    pub fn asked(&self) -> Vec<Asked> {
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn urls(&self) -> Vec<String> {
        self.asked().into_iter().map(|asked| asked.url).collect()
    }
}

impl HttpClient for RecordingHttp {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, HttpError> {
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(Asked {
                url: request.url().to_string(),
                with_key: request.has_key(),
                headers: request
                    .headers()
                    .into_iter()
                    .map(|(name, _)| name)
                    .collect(),
            });
        if request.timeout.is_zero() {
            return Err(HttpError::Timeout);
        }
        let mut answer = (self.respond)(request)?;
        if !answer.is_ok() {
            answer.body = Ok(Vec::new());
        }
        if answer
            .body
            .as_ref()
            .is_ok_and(|body| body.len() as u64 > request.limit)
        {
            answer.body = Err(TooLarge::Read);
        }
        Ok(answer)
    }
}

pub fn status(status: u16) -> HttpResponse {
    HttpResponse {
        status,
        content_type: None,
        location: None,
        body: Ok(Vec::new()),
    }
}

pub fn json(status: u16, text: &str) -> HttpResponse {
    HttpResponse {
        status,
        content_type: Some("application/json".to_owned()),
        location: None,
        body: Ok(text.as_bytes().to_vec()),
    }
}

pub fn picture(content_type: &str, bytes: Vec<u8>) -> HttpResponse {
    HttpResponse {
        status: 200,
        content_type: Some(content_type.to_owned()),
        location: None,
        body: Ok(bytes),
    }
}

pub fn redirect(status: u16, location: &str) -> HttpResponse {
    HttpResponse {
        status,
        content_type: None,
        location: Some(location.to_owned()),
        body: Ok(Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use ol_platform_api::http::ArtworkUrl;

    use super::*;

    const WAIT: Duration = Duration::from_secs(15);

    #[test]
    fn a_request_is_written_down_without_its_key_and_answered_as_the_real_client_would() {
        let http = RecordingHttp::new(|request| {
            Ok(match request.url().host() {
                "www.steamgriddb.com" => json(401, "{\"success\":false}"),
                _ => picture("image/png", vec![7; 9]),
            })
        });
        let api = HttpRequest::lookup("https://www.steamgriddb.com/api/v2/grids/steam/1", 64, WAIT)
            .unwrap()
            .with_key("placeholderkey0000000000")
            .unwrap();
        let refused = http.send(&api).unwrap();
        assert_eq!((refused.status, refused.body), (401, Ok(Vec::new())));

        let url = ArtworkUrl::image("https://cdn2.steamgriddb.com/grid/a.png").unwrap();
        let fits = HttpRequest::image(url.clone(), 9, WAIT).unwrap();
        assert_eq!(http.send(&fits).unwrap().body, Ok(vec![7; 9]));
        let over = HttpRequest::image(url.clone(), 8, WAIT).unwrap();
        assert_eq!(http.send(&over).unwrap().body, Err(TooLarge::Read));
        let spent = HttpRequest::image(url, 9, Duration::ZERO).unwrap();
        assert_eq!(http.send(&spent), Err(HttpError::Timeout));

        let asked = http.asked();
        assert_eq!(asked.len(), 4);
        assert_eq!(
            asked[0],
            Asked {
                url: "https://www.steamgriddb.com/api/v2/grids/steam/1".to_owned(),
                with_key: true,
                headers: vec![
                    "accept",
                    "accept-language",
                    "sec-fetch-mode",
                    "user-agent",
                    "authorization"
                ],
            }
        );
        assert!(!asked[1].with_key);
        assert!(!format!("{asked:?}").contains("placeholderkey"));
    }
}
