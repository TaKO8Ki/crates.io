//! All routes related to managing owners of a crate

use crate::controllers::prelude::*;
use crate::models::{Crate, Owner, Rights, Team, User};
use crate::views::EncodableOwner;

/// Handles the `GET /crates/:crate_id/owners` route.
pub fn owners(req: &mut dyn RequestExt) -> EndpointResult {
    let crate_name = &req.params()["crate_id"];
    let conn = req.db_read_only()?;
    let krate: Crate = Crate::by_name(crate_name).first(&*conn)?;
    let owners = krate
        .owners(&conn)?
        .into_iter()
        .map(Owner::into)
        .collect::<Vec<EncodableOwner>>();

    Ok(req.json(&json!({ "users": owners })))
}

/// Handles the `GET /crates/:crate_id/owner_team` route.
pub fn owner_team(req: &mut dyn RequestExt) -> EndpointResult {
    let crate_name = &req.params()["crate_id"];
    let conn = req.db_read_only()?;
    let krate: Crate = Crate::by_name(crate_name).first(&*conn)?;
    let owners = Team::owning(&krate, &conn)?
        .into_iter()
        .map(Owner::into)
        .collect::<Vec<EncodableOwner>>();

    Ok(req.json(&json!({ "teams": owners })))
}

/// Handles the `GET /crates/:crate_id/owner_user` route.
pub fn owner_user(req: &mut dyn RequestExt) -> EndpointResult {
    let crate_name = &req.params()["crate_id"];
    let conn = req.db_read_only()?;
    let krate: Crate = Crate::by_name(crate_name).first(&*conn)?;
    let owners = User::owning(&krate, &conn)?
        .into_iter()
        .map(Owner::into)
        .collect::<Vec<EncodableOwner>>();

    Ok(req.json(&json!({ "users": owners })))
}

/// Handles the `PUT /crates/:crate_id/owners` route.
pub fn add_owners(req: &mut dyn RequestExt) -> EndpointResult {
    modify_owners(req, true)
}

/// Handles the `DELETE /crates/:crate_id/owners` route.
pub fn remove_owners(req: &mut dyn RequestExt) -> EndpointResult {
    modify_owners(req, false)
}

/// Parse the JSON request body of requests to modify the owners of a crate.
/// The format is
///
///     {"owners": ["username", "github:org:team", ...]}
fn parse_owners_request(req: &mut dyn RequestExt) -> AppResult<Vec<String>> {
    let mut body = String::new();
    req.body().read_to_string(&mut body)?;
    #[derive(Deserialize)]
    struct Request {
        // identical, for back-compat (owners preferred)
        users: Option<Vec<String>>,
        owners: Option<Vec<String>>,
    }
    let request: Request =
        serde_json::from_str(&body).map_err(|_| cargo_err("invalid json request"))?;
    request
        .owners
        .or(request.users)
        .ok_or_else(|| cargo_err("invalid json request"))
}

fn modify_owners(req: &mut dyn RequestExt, add: bool) -> EndpointResult {
    let authenticated_user = req.authenticate()?;
    let logins = parse_owners_request(req)?;
    let app = req.app();
    let crate_name = &req.params()["crate_id"];

    let conn = req.db_conn()?;
    let user = authenticated_user.user();

    conn.transaction(|| {
        let krate: Crate = Crate::by_name(crate_name).first(&*conn)?;
        let owners = krate.owners(&conn)?;

        match user.rights(app, &owners)? {
            Rights::Full => {}
            // Yes!
            Rights::Publish => {
                return Err(cargo_err(
                    "team members don't have permission to modify owners",
                ));
            }
            Rights::None => {
                return Err(cargo_err("only owners have permission to modify owners"));
            }
        }

        let comma_sep_msg = if add {
            let mut msgs = Vec::with_capacity(logins.len());
            for login in &logins {
                let login_test =
                    |owner: &Owner| owner.login().to_lowercase() == *login.to_lowercase();
                if owners.iter().any(login_test) {
                    return Err(cargo_err(&format_args!("`{}` is already an owner", login)));
                }
                let msg = krate.owner_add(app, &conn, &user, login)?;
                msgs.push(msg);
            }
            msgs.join(",")
        } else {
            for login in &logins {
                krate.owner_remove(app, &conn, &user, login)?;
            }
            if User::owning(&krate, &conn)?.is_empty() {
                return Err(cargo_err(
                    "cannot remove all individual owners of a crate. \
                     Team member don't have permission to modify owners, so \
                     at least one individual owner is required.",
                ));
            }
            "owners successfully removed".to_owned()
        };

        Ok(req.json(&json!({ "ok": true, "msg": comma_sep_msg })))
    })
}
