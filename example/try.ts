import * as _ from "lodash-es"
_.each([1, 2, 3], (n) => {
    console.log(n)
})

const __ = require("lodash")

const mean = __.mean([4, 5, 6])
console.log("Mean is:", mean)

const sleep = (x: number, f: () => void = () => { }) => {
    return new Promise<void>((resolve) => {
        const timeout = setTimeout(() => {
            clearTimeout(timeout)
            resolve(f())
        }, x);
    });
}

await sleep(1000, () => { console.log("Waited 1 second") })