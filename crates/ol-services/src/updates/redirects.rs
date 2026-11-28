use ol_platform_api::update_http::UpdateRequest;

use super::failure::UpdateFailure;

pub const MOST_REDIRECTS: usize = 5;

pub(crate) struct Hop<T> {
    pub status: u16,
    pub location: Option<String>,
    pub answer: T,
}

const fn is_redirect(status: u16) -> bool {
    status >= 300 && status <= 399
}

pub(crate) fn follow<T, E: From<UpdateFailure>>(
    first: UpdateRequest,
    mut ask: impl FnMut(&UpdateRequest) -> Result<Hop<T>, E>,
) -> Result<(u16, T), E> {
    let mut request = first;
    let mut followed = 0;
    loop {
        let hop = ask(&request)?;
        if !is_redirect(hop.status) {
            return Ok((hop.status, hop.answer));
        }
        let next = hop
            .location
            .as_deref()
            .filter(|_| followed < MOST_REDIRECTS)
            .and_then(|location| request.redirected(location))
            .ok_or(UpdateFailure::Redirected)?;
        request = next;
        followed += 1;
    }
}
