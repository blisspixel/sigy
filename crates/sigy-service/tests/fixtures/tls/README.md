# TLS rejection fixture

`untrusted-cert.der` is a self-signed RSA certificate for `fixture.invalid`.
`fixture-key.der` is its deliberately public PKCS#8 test key. These bytes serve
only the loopback TLS rejection test and grant no access to any service.
Never install the certificate into a trust store or use this key operationally.

The fixture was created locally with the .NET cryptography API. To regenerate
both files together, run PowerShell from this directory:

```powershell
$fixtureKey = [Security.Cryptography.RSA]::Create(2048)
$fixtureRequest = [Security.Cryptography.X509Certificates.CertificateRequest]::new(
    'CN=fixture.invalid', $fixtureKey,
    [Security.Cryptography.HashAlgorithmName]::SHA256,
    [Security.Cryptography.RSASignaturePadding]::Pkcs1)
$fixtureNames = [Security.Cryptography.X509Certificates.SubjectAlternativeNameBuilder]::new()
$fixtureNames.AddDnsName('fixture.invalid')
$fixtureRequest.CertificateExtensions.Add($fixtureNames.Build())
$fixtureCertificate = $fixtureRequest.CreateSelfSigned(
    [DateTimeOffset]::UtcNow.AddDays(-1), [DateTimeOffset]::UtcNow.AddYears(10))
[IO.File]::WriteAllBytes((Join-Path (Get-Location) 'untrusted-cert.der'),
    $fixtureCertificate.Export([Security.Cryptography.X509Certificates.X509ContentType]::Cert))
[IO.File]::WriteAllBytes((Join-Path (Get-Location) 'fixture-key.der'),
    $fixtureKey.ExportPkcs8PrivateKey())
$fixtureCertificate.Dispose()
$fixtureKey.Dispose()
```

Tests use retained DER files and require no certificate generator or external
server. The test verifies rejection, not trusted-certificate interoperability on
every platform. Keep the fixture and test expectations aligned when rotating it.
