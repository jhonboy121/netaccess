use crate::user::User;
use anyhow::{anyhow, bail, Context, Result};
use chrono::{Duration, FixedOffset, NaiveDateTime, Utc};
use reqwest::{tls::Version, Client, ClientBuilder, Response};
use scraper::{ElementRef, Html, Selector};
use std::{collections::HashMap, net::IpAddr};

const URL: &str = "https://netaccess.iitm.ac.in";
const LOGIN_PATH: &str = "/account/login";
const INDEX_PATH: &str = "/account/index";
const APPROVE_PATH: &str = "/account/approve";
const REVOKE_PATH: &str = "/account/revoke";

const USER_NAME_FIELD: &str = "userLogin";
const PASSWORD_FIELD: &str = "userPassword";

const DURATION_FIELD: &str = "duration";
const APPROVE_BTN_FIELD: &str = "approveBtn";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connection {
    time_left: Duration,
    is_active: bool,
}

impl Connection {
    pub fn time_left(&self) -> &Duration {
        &self.time_left
    }

    pub fn is_active(&self) -> bool {
        !self.time_left.is_zero() && self.is_active
    }
}

#[derive(Debug, Clone)]
pub struct Status {
    system_connection: (IpAddr, Connection),
    connections: HashMap<IpAddr, Connection>,
}

impl Status {
    pub fn system_connection(&self) -> &(IpAddr, Connection) {
        &self.system_connection
    }

    pub fn connections(&self) -> &HashMap<IpAddr, Connection> {
        &self.connections
    }

    fn is_connection_active(&self, ip: &IpAddr) -> bool {
        self.connections
            .get(ip)
            .is_some_and(|connection| connection.is_active())
    }
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("HTTP request error encountered during an operation: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("Invalid user credentials")]
    InvalidCredentials,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub struct AccountManager {
    client: Client,
}

impl AccountManager {
    pub fn new() -> reqwest::Result<Self> {
        ClientBuilder::default()
            .min_tls_version(Version::TLS_1_2)
            .cookie_store(true)
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .map(|client| Self { client })
    }

    pub async fn status(&self, user: &User) -> Result<Status, Error> {
        self.login(user).await?;
        let ip = local_ip_address::local_ip().context("Failed to get local ip address")?;
        let index_page = self.index_page_response().await?;
        let html = index_page.text().await?;
        let mut connections = Self::parse_connections(&html)?;
        let system_connection = (
            ip,
            connections.remove(&ip).unwrap_or_else(|| Connection {
                time_left: Duration::zero(),
                is_active: false,
            }),
        );
        Ok(Status {
            system_connection,
            connections,
        })
    }

    async fn login(&self, user: &User) -> Result<(), Error> {
        if self.is_logged_in().await? {
            return Ok(());
        }
        let login_form = HashMap::from([
            (USER_NAME_FIELD, user.name()),
            (PASSWORD_FIELD, user.password()),
        ]);
        let response = self
            .client
            .post(format!("{URL}{LOGIN_PATH}"))
            .form(&login_form)
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(Error::Other(anyhow!(
                "Login response failed with status {}",
                response.status()
            )));
        }
        match response.url().path() {
            INDEX_PATH => Ok(()),
            LOGIN_PATH => Err(Error::InvalidCredentials),
            other => Err(Error::Other(anyhow!(
                "Unexpected URL path in login response {other}"
            ))),
        }
    }

    async fn is_logged_in(&self) -> Result<bool> {
        let response = self.index_page_response().await?;
        if !response.status().is_success() {
            bail!("Index page response is not a success: {response:?}");
        }
        Ok(response.url().path() == INDEX_PATH)
    }

    async fn index_page_response(&self) -> reqwest::Result<Response> {
        self.client.get(format!("{URL}{INDEX_PATH}")).send().await
    }

    fn parse_connections(html: &str) -> Result<HashMap<IpAddr, Connection>> {
        let html = Html::parse_document(html);
        let tbody_selector = Selector::parse("tbody").expect("Failed to create tbody selector");
        let Some(tbody) = html.select(&tbody_selector).next() else {
            bail!("Html does not have a tbody element")
        };
        let mut connections = HashMap::with_capacity(tbody.children().count() - 1);
        /*
        <tbody>
            <tr>
                <th>MAC</th>
                <th align="center">IP</th>
                <th>Valid till</th>
                <th>Download today</th>
                <th colspan="2">Status</th>
            </tr>
            <tr>
                <td>
                    <E:XP:IR:ED:>
                </td>
                <td>XX.XX.XX.XXX</td>
                <td>24 Jul 2023, 10:07</td>
                <td>     0 B</td>
                <td><span class='label label-success'>Active</span></td>
                <td><a href="/account/revoke/XX.XX.XX.XXX"><span class='label label-danger'>Delete</span></a></td>
            </tr>
        </tbody>
         */

        let tr_selector = Selector::parse("tr").expect("Failed to create tr selector");
        let td_selector = Selector::parse("td").expect("Failed to create td selector");
        let span_selector = Selector::parse("span").expect("Failed to create span selector");

        let india_tz =
            FixedOffset::east_opt((5 * 3600) + (30 * 60)).expect("Failed to create India timezone");
        let now = Utc::now().with_timezone(&india_tz).naive_local();

        // First tr element is a header, so we skip it
        for tr_element in tbody.select(&tr_selector).skip(1) {
            let td_elements = tr_element.select(&td_selector);
            // First td element is of no use either.
            let mut td_elements = td_elements.skip(1);
            let Some(ip_element) = td_elements.next() else {
                bail!("Missing ip address element");
            };
            let ip_address =
                Self::extract_text(ip_element).context("Extracting IP address failed")?;
            let Some(validity_element) = td_elements.next() else {
                bail!("Missing validity element");
            };
            let validity =
                Self::extract_text(validity_element).context("Extracting status failed")?;
            let Some(span) = tr_element.select(&span_selector).next() else {
                bail!("Missing status element");
            };
            let status = Self::extract_text(span)?;
            let validity = NaiveDateTime::parse_from_str(&validity, "%d %b %Y, %H:%M")?;
            connections.insert(
                ip_address.parse()?,
                Connection {
                    time_left: validity - now,
                    is_active: &status == "Active",
                },
            );
        }
        Ok(connections)
    }

    fn extract_text(element: ElementRef<'_>) -> Result<String> {
        let Some(text_node) = element.children().next() else {
            bail!("Malfored element");
        };
        let Some(text) = text_node.value().as_text() else {
            bail!("Malfored text node");
        };
        Ok(text.to_string())
    }

    pub async fn approve(
        &self,
        user: &User,
        duration_index: usize,
        force: bool,
    ) -> Result<IpAddr, Error> {
        let status = self.status(user).await?;

        let (ip, connection) = status.system_connection();

        if !force && connection.is_active() {
            return Ok(*ip);
        }

        let approve_form = HashMap::from([
            (DURATION_FIELD, duration_index.to_string()),
            (APPROVE_BTN_FIELD, String::new()),
        ]);

        let response = self
            .client
            .post(format!("{URL}{APPROVE_PATH}"))
            .form(&approve_form)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(Error::Other(anyhow!(
                "Approve response failed with status {}",
                response.status()
            )));
        }
        match response.url().path() {
            INDEX_PATH => Ok(*ip),
            other => Err(Error::Other(anyhow!(
                "Unexpected URL path in approve response {other}"
            ))),
        }
    }

    pub async fn revoke(&self, user: &User, ip: Option<String>) -> Result<IpAddr, Error> {
        let status = self.status(user).await?;

        let ip = match ip {
            Some(ip) => ip
                .parse()
                .with_context(|| format!("Ip address is malformed {ip}"))?,
            None => local_ip_address::local_ip().context("Failed to get local ip address")?,
        };

        if !status.is_connection_active(&ip) {
            return Ok(ip);
        }

        let response = self
            .client
            .post(format!("{URL}{REVOKE_PATH}/{ip}"))
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(Error::Other(anyhow!(
                "Revoke response failed with status {}",
                response.status()
            )));
        }
        match response.url().path() {
            INDEX_PATH => Ok(ip),
            other => Err(Error::Other(anyhow!(
                "Unexpected URL path in revoke response {other}"
            ))),
        }
    }
}
