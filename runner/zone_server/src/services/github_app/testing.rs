//! Fixtures shared by this module's tests.

use chrono::{DateTime, Utc};

/// A throwaway RSA key, generated for these tests and used nowhere else.
pub const TEST_PRIVATE_KEY: &str = "-----BEGIN RSA PRIVATE KEY-----
MIIEogIBAAKCAQEAu5dqhjvoHZtbdSz9bGJAMp3r+1bqzy/9lhjVz5crI2yWhz7I
z3M0H74MQPCHhlOn8nyeOs1cclBLRt7iX7/wgkaP9mGGpvCzLXYdLH4JrrMZj7r5
Pjt0KHrrouTvYXjuCTu2F6QnBcIe+Esz5Vs/+V2fa73I+M1C0sHdX22YNIazcAcB
I9gnaEeS+gINe8LUZDf72lJQc4z+gmVb7dd2R9YEAc90NPpkoeGl7pFwrVHRFlsY
AbJAz6pKc1dVkfTU/yFuw/0d0117InbS1RxTVFEunMWqDJLFwl01THIuVzLDtX93
8cmeMZMcys+n74ZNoA9W6Pv9P2GVQsbnnZV43QIDAQABAoIBAAj9cOq1cMKIeTnU
meGJlNuII3DEYdTjjyLUFl0QOM5GDDG3kc6VTgxuYm5zSH9ov234yGl3gYRt8imX
kWA21dsccBZF5rrV4rRdSnkhIiwn46P2eS7hEQhGmcfQ8mLotXmmTav03zTgsHTE
P9ywQpDcCoGSkxDPX3IzvbzuxuJPdBIIZLraG07haKpemdvNR4UoeMl8blAic3df
NjcFPVD8t4my7EZTgHJtWDqYTW/g/EPwIeMZ/DOEqHDNey9RrKTi4tAr9Uzsszg0
YK2mEFr2B2NG/G5/1X5fh/QV7tPvbKXxFMCiL7RBv9LW3qD8LTs/ovQUxEgOBspV
qyAuCwECgYEA+5Y5Xr0/uedzoGpMLhg86aHdt80cX+Ssi5HkKXwJa5taYCz0gbgU
r7s/9hlGVLiXEjwnXmhWXETq/56b4ebO0u8fcM4ptLrnr+KKWs3AXdjtSvTskOkH
S4QCx6VuZpetyVD6yLZucVes07S6bfIIcRNQto3DPcBCYm+YsNHSdd0CgYEAvuHQ
gazZaxRSTap8IAiRZ2mHUxK3FkVqWrR1aXG/V7OsZprDRVzHz662f+5a27ZCTc4q
2tC8B04tUA0AFWYpk7wh4vnb2VffziarFnCRXhOeOK9/82vxsMwOEX8qgRHs0GK0
1zHTFqUA/lsA6bcqQZYmOn4r0dSPJMahp5i+XwECgYB/7/nGqrhwYjnTdpq8ygiX
yn+Ei2KFhTUVWKBNVE06EmtYAyRnnuOuJau2C05PoPr6A+sFQEvCai2SxeaBbyz3
6S/03nIo/O7661nuKTlMwBaTio+OdWIHTd9YBVFqDHIMsQiG7vak3q/9jKdNZ8pR
LkBaRSbnDRD1G8jrChhbZQKBgFkaD7p4dQUG92RJsKdDWJxtJj4g/lXnET5F/oi6
EBdgR5mdpIk8RgksBQSyvrbQ3SJ0moyJ4zuFwqEbcG6Mwdu0dhz9hSJvYolYg4R2
B2Viwviy84ctXCSrG+YO9khJlcGwUboiB+cKHuycjlCKr67t5+pl+w53qloAXnVd
V4ABAoGAJQaRxv9kpn6TMTCDtTPtWpVyxymF0A58t8/Yy3lcC3HS3V5hDRZlBGyL
snLljJnOKrZGf+JfNkPul+la0f9cJo9Ka/oCy6X243y8VmzRG6/BX6eEfnWEJ+NB
+ehfHUDVhe3oJRr15Rw1gt7CrSORoe46YkbnxO3H80of1gn9Vyw=
-----END RSA PRIVATE KEY-----";

/// A fixed instant, so a test asserting on `iat` and `exp` cannot flake.
pub fn at(timestamp: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(timestamp, 0).expect("timestamp in range")
}
