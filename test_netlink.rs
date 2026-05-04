use futures::TryStreamExt;
use rtnetlink::new_connection;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (conn, handle, _) = new_connection()?;
    tokio::spawn(conn);

    let iface_idx = get_interface_index(&handle, "wlp0s20f3").await?;
    println!("Interface index: {}", iface_idx);

    let mut addresses = handle
        .address()
        .get()
        .set_link_index_filter(iface_idx)
        .execute();

    while let Some(msg) = addresses.try_next().await? {
        println!("Family: {}", msg.header.family);
        for attr in &msg.nlas {
            println!("  NLA: {:?}", attr);
        }
    }

    Ok(())
}

async fn get_interface_index(handle: &rtnetlink::Handle, name: &str) -> Result<u32, Box<dyn std::error::Error>> {
    use futures::TryStreamExt;
    let mut links = handle.link().get().match_name(name.to_string()).execute();
    if let Some(msg) = links.try_next().await? {
        return Ok(msg.header.index);
    }
    Err("Interface not found".into())
}
