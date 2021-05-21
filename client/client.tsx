// minimal client
// assumes justsql is routed to the /justsql path on the
// same endpoint as this system.

// modify root to point to wherver your justsql endpoints are routed to.
// for CORS you'll need to set ROOT to the domain.
// Example if justsql is hosted at `justsql.api.example.com` set ROOT to `https://justsql.api.example.com/`
// Or if justsql is proxied through `/justsql/:path*` then you should set root to be `/justsql/`
const ROOT = "/";

export type Resp<A> =
    | {
        status: "error";
        endpoint: string;
        message: string;
    }
    | {
        status: "success";
        endpoint: string;
        data: A;
    };

function getFirst<A>(resp: Resp<A[]>): Resp<A> {
    if (resp.status === "error") {
        return resp;
    } else {
        return {
            status: resp.status,
            endpoint: resp.endpoint,
            data: resp.data[0],
        };
    }
}

export async function auth(
    endpoint: string,
    payload: { [key: string]: any }
): Promise<Resp<string>> {
    return fetch(`${ROOT}/api/v1/auth`, {
        method: "POST",
        credentials: "include",
        headers: {
            "Content-Type": "application/json",
        },
        body: JSON.stringify({ endpoint, payload }),
    }).then((resp) => resp.json());
}

export async function query<A>(
    endpoint: string,
    payload: { [key: string]: any }
): Promise<Resp<A[]>> {
    return fetch(`${ROOT}/api/v1/query`, {
        method: "POST",
        credentials: "include",
        headers: {
            "Content-Type": "application/json",
        },
        body: JSON.stringify([{ endpoint, payload }]),
    }).then(async (resp) => (await resp.json())[0]);
}

export async function queryFirst<A>(
    endpoint: string,
    payload: { [key: string]: any }
): Promise<Resp<A>> {
    return query<A>(endpoint, payload).then(getFirst);
}
