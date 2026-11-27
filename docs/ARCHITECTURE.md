# Architecture

## Crates

The `*-proto` crates and `denon-marantz-discovery` have no IO. They parse and
build messages. The other crates do the IO with tokio.

Dashed arrows are optional dependencies. The label lists the features of
`denon-marantz` that turn them on directly. Any one of the listed features is
enough. `discovery`, `heos` and `mdns` turn on `http`, and `mdns` turns on
`discovery`.

```mermaid
flowchart TD
    dm[denon-marantz]
    proto[denon-marantz-proto]
    disc[denon-marantz-discovery]
    heos[denon-marantz-heos]
    heosproto[denon-marantz-heos-proto]
    shared[denon-marantz-shared]

    dm --> proto
    dm -.->|discovery| disc
    dm -.->|heos| heos
    dm -.->|"telnet, http"| shared
    heos --> heosproto
    heos --> shared
```

## Data flow in `denon-marantz`

1. `discover` finds receivers with SSDP. With the `mdns` feature,
   `discover_all` uses mDNS too. The program passes the host of a found
   receiver to `Avr::builder`.
2. `AvrBuilder::connect` reads `Deviceinfo.xml` and the device description
   over HTTP to find the kind of receiver, its HTTP port, brand and model.
   With `AvrBuilder::known` it sends no request.
3. `Avr::start` reads the zones over HTTP. Telnet and the other HTTP reads
   fill in the rest in the background.
4. Every receiver value goes through `Channels`, which keeps the `State` and
   sends a `Change` for each change. HEOS has its own state and its own
   changes.
5. Commands use telnet while it is open, and HTTP otherwise.

`discover` needs the `discovery` feature, the telnet task needs `telnet`, and
`Avr::heos` needs `heos` and a receiver with HEOS.

```mermaid
flowchart LR
    subgraph receiver[Receiver]
        ssdp["SSDP 1900 / mDNS"]
        http["HTTP: AppCommand, status pages,<br/>web interface 11080"]
        telnet["Telnet 23"]
        heosport["HEOS 1255"]
    end

    subgraph dm[denon-marantz]
        discover["discover"]
        builder["AvrBuilder::connect<br/>(identify)"]
        avr["Avr"]
        controls["Controls: zone, sound,<br/>device, tone, tuner, video,<br/>audyssey, dirac"]
        httpclient["HttpClient<br/>(Pacer, Priority)"]
        telnettask["Telnet task"]
        channels["Channels"]
        heoslink["HeosLink"]
    end

    subgraph proto[denon-marantz-proto]
        parsers["AppCommand and page parsers"]
        decoder["LineDecoder, Message"]
        state["State::apply_*"]
    end

    ssdp --> discover
    discover --> httpclient
    discover -->|"host"| builder
    builder --> avr
    builder --> httpclient

    controls --> avr
    avr -->|"reads; send: no telnet"| httpclient
    avr -->|"send: telnet open"| telnettask

    httpclient <--> http
    telnettask <--> telnet
    heoslink <--> heosport

    httpclient --> parsers --> state
    telnettask --> decoder --> state
    state --> channels

    channels -->|"state, snapshot, changes"| app([Program])
    avr --> heoslink
    heoslink -->|"heos, heos_changes"| app
```
