use anyhow::Result;
use bollard::Docker;

/// Connect to Docker daemon with appropriate method based on the host URL
pub async fn connect_docker(docker_host: &str) -> Result<Docker> {
    let docker = match docker_host {
        s if s.starts_with("unix://") => {
            Docker::connect_with_unix(s, 120, bollard::API_DEFAULT_VERSION)?
        }
        s if s.starts_with("tcp://") || s.starts_with("http://") || s.starts_with("https://") => {
            Docker::connect_with_http(s, 120, bollard::API_DEFAULT_VERSION)?
        }
        _ => Docker::connect_with_local_defaults()?,
    };
    Ok(docker)
}
