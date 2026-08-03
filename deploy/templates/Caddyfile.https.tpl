# Template variables: __PANEL_DOMAIN__ __DATABASUS_DOMAIN__ __EMAIL__ __DATABASUS_PUBLIC__
{
	email __EMAIL__
}

__PANEL_DOMAIN__ {
	encode zstd gzip
	header {
		Strict-Transport-Security "max-age=31536000; includeSubDomains; preload"
		X-Content-Type-Options "nosniff"
		X-Frame-Options "DENY"
		Referrer-Policy "strict-origin-when-cross-origin"
		Permissions-Policy "geolocation=(), microphone=(), camera=()"
		Content-Security-Policy "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; frame-ancestors 'none'; base-uri 'self'; form-action 'self'"
		-Server
	}
	request_body {
		max_size 10MB
	}
	reverse_proxy panel:8080
	log {
		output stdout
		format console
	}
}

# Include this vhost only when __DATABASUS_PUBLIC__=1.
__DATABASUS_DOMAIN__ {
	encode zstd gzip
	header {
		Strict-Transport-Security "max-age=31536000; includeSubDomains; preload"
		X-Content-Type-Options "nosniff"
		X-Frame-Options "DENY"
		Referrer-Policy "strict-origin-when-cross-origin"
		-Server
	}
	reverse_proxy databasus:4005
	log {
		output stdout
		format console
	}
}
