use crate::user::User;
use anyhow::{anyhow, bail, Context};
use chrono::{FixedOffset, NaiveDateTime, Utc};
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

lazy_static::lazy_static! {
    static ref INDIA_TZ: FixedOffset =
        FixedOffset::east_opt(5 * 3600 + 30 * 60).expect("Failed to create India timezone");

    static ref TBODY_SELECTOR: Selector =
        Selector::parse("tbody").expect("Failed to create tbody selector");
    static ref TR_SELECTOR: Selector =
        Selector::parse("tr").expect("Failed to create tr selector");
    static ref TD_SELECTOR: Selector =
        Selector::parse("td").expect("Failed to create td selector");
    static ref SPAN_SELECTOR: Selector =
        Selector::parse("span").expect("Failed to create span selector");
}

#[derive(Debug, Clone, Copy)]
pub struct Connection {
    pub time_left: chrono::Duration,
    is_active: bool,
}

impl Default for Connection {
    fn default() -> Self {
        Self {
            time_left: chrono::Duration::zero(),
            is_active: false,
        }
    }
}

impl Connection {
    pub fn is_active(&self) -> bool {
        !self.time_left.is_zero() && self.is_active
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SystemStatus {
    pub ip: IpAddr,
    pub connection: Connection,
}

#[derive(Debug, Clone)]
pub struct Status {
    pub system_status: SystemStatus,
    connections: HashMap<IpAddr, Connection>,
}

impl Status {
    pub fn connections(&self) -> &HashMap<IpAddr, Connection> {
        &self.connections
    }

    fn is_connection_active(&self, ip: &IpAddr) -> bool {
        self.connections.get(ip).is_some_and(Connection::is_active)
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

#[derive(Debug)]
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

    pub async fn check_user_passowrd(&self, user: &User) -> Result<(), Error> {
        self.login(user, true).await
    }

    fn local_ip() -> anyhow::Result<IpAddr> {
        local_ip_address::local_ip().context("Failed to get local ip address")
    }

    pub async fn status(&self, user: &User) -> Result<Status, Error> {
        self.login(user, false).await?;
        let index_page = self.index_page_response().await?;
        let html = index_page.text().await?;
        let mut connections = Self::parse_connections(&html)?;
        let ip = Self::local_ip()?;
        let system_connection = SystemStatus {
            ip,
            connection: connections.remove(&ip).unwrap_or_default(),
        };
        Ok(Status {
            system_status: system_connection,
            connections,
        })
    }

    async fn login(&self, user: &User, force: bool) -> Result<(), Error> {
        if !force && self.is_logged_in().await? {
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

    async fn is_logged_in(&self) -> anyhow::Result<bool> {
        let response = self.index_page_response().await?;
        if response.status().is_success() {
            Ok(response.url().path() == INDEX_PATH)
        } else {
            Err(anyhow!(
                "Index page response is not a success: {response:?}"
            ))
        }
    }

    async fn index_page_response(&self) -> reqwest::Result<Response> {
        self.client.get(format!("{URL}{INDEX_PATH}")).send().await
    }

    fn time_now() -> NaiveDateTime {
        Utc::now().with_timezone(&*INDIA_TZ).naive_local()
    }

    fn extract_text(element: ElementRef) -> Option<String> {
        element
            .children()
            .next()
            .and_then(|node| node.value().as_text())
            .map(|text| text.to_string())
    }

    fn parse_tr_element(tr_element: ElementRef) -> anyhow::Result<(IpAddr, Connection)> {
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

        let td_elements = tr_element.select(&TD_SELECTOR);
        // First td element is of no use either.
        let mut td_elements = td_elements.skip(1);

        let Some(ip_element) = td_elements.next() else {
            bail!("Missing IP address element");
        };
        let ip_address = Self::extract_text(ip_element).context("Extracting IP address failed")?;

        let Some(valid_till_element) = td_elements.next() else {
            bail!("Missing remaining duration element");
        };
        let valid_till = Self::extract_text(valid_till_element)
            .context("Extracting remaining duration failed")?;
        let valid_till = NaiveDateTime::parse_from_str(&valid_till, "%d %b %Y, %H:%M")?;

        let Some(status_element) = tr_element.select(&SPAN_SELECTOR).next() else {
            bail!("Missing status element");
        };
        let status = Self::extract_text(status_element).context("Extracting status failed")?;

        Ok((
            ip_address.parse::<IpAddr>()?,
            Connection {
                time_left: chrono::Duration::max(
                    chrono::Duration::zero(),
                    valid_till - Self::time_now(),
                ),
                is_active: &status == "Active",
            },
        ))
    }

    fn parse_connections(html: &str) -> anyhow::Result<HashMap<IpAddr, Connection>> {
        let html = Html::parse_document(html);
        let Some(tbody) = html.select(&TBODY_SELECTOR).next() else {
            bail!("Html does not have a tbody element")
        };
        tbody
            .select(&TR_SELECTOR)
            .skip(1)
            .map(Self::parse_tr_element)
            .collect()
    }

    pub async fn approve(
        &self,
        user: &User,
        duration_index: usize,
        force: bool,
    ) -> Result<IpAddr, Error> {
        let status = self.status(user).await?;

        let SystemStatus { ip, connection } = status.system_status;

        if !force && connection.is_active() {
            return Ok(ip);
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
            INDEX_PATH => Ok(ip),
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
            None => Self::local_ip()?,
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
