// minimal client 
// assumes justsql is routed to the /justsql path on the 
// same endpoint as this system.

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

export async function auth<A>(
    endpoint: string,
    payload: { [key: string]: any }
): Promise<Resp<A>> {
    return fetch(`/justsql/api/v1/auth`, {
        method: "POST",
        headers: {
            "Content-Type": "application/json",
        },
        body: JSON.stringify({ endpoint, payload }),
    }).then((resp) => resp.json());
}

export async function query<A>(
    endpoint: string,
    payload: { [key: string]: any }
): Promise<Resp<A>> {
    return fetch(`/justsql/api/v1/query`, {
        method: "POST",
        headers: {
            "Content-Type": "application/json",
        },
        body: JSON.stringify([{ endpoint, payload }]),
    }).then(async (resp) => (await resp.json())[0]);
}
