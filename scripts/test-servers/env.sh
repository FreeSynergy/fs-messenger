# Shared settings for the FreeSynergy messenger test servers.
# Source this file from a shell, or read it from a test, to find the servers.
#
# ALL PASSWORDS AND TOKENS BELOW ARE THROWAWAY TEST VALUES. They protect nothing
# but disposable, loopback-only containers. Never reuse them anywhere else.

# --- Names. Everything we create carries the prefix fs-msgtest-. ---
FS_MSGTEST_STALWART_CONTAINER=fs-msgtest-stalwart
FS_MSGTEST_STALWART_VOLUME_ETC=fs-msgtest-stalwart-etc
FS_MSGTEST_STALWART_VOLUME_DATA=fs-msgtest-stalwart-data
FS_MSGTEST_TUWUNEL_CONTAINER=fs-msgtest-tuwunel
FS_MSGTEST_TUWUNEL_VOLUME_DATA=fs-msgtest-tuwunel-data

# --- Images, pinned by digest. The tags are recorded for humans only. ---
FS_MSGTEST_STALWART_VERSION=1.0.3
FS_MSGTEST_STALWART_DIGEST=sha256:73c684a84c24f5b4206ec266a8deb8986fe44a9f623997e07f084388d943c8bd
FS_MSGTEST_STALWART_IMAGE=docker.io/stalwartlabs/stalwart@$FS_MSGTEST_STALWART_DIGEST
FS_MSGTEST_TUWUNEL_VERSION=1.9.4
FS_MSGTEST_TUWUNEL_DIGEST=sha256:0e86c6164d6c9f5b60291938eb61ad8395021c37fe58d47a963ea7edddbedaec
FS_MSGTEST_TUWUNEL_IMAGE=docker.io/jevolk/tuwunel@$FS_MSGTEST_TUWUNEL_DIGEST

# --- Loopback ports on the host. ---
FS_MSGTEST_HOST=127.0.0.1
FS_MSGTEST_SMTP_PORT=18465   # SMTP submission, implicit TLS (container port 465)
FS_MSGTEST_IMAP_PORT=18993   # IMAP, implicit TLS (container port 993)
FS_MSGTEST_JMAP_PORT=18480   # JMAP and management API, plain HTTP (container port 8080)
FS_MSGTEST_HTTPS_PORT=18443  # JMAP over HTTPS, self-signed (container port 443)
FS_MSGTEST_MATRIX_PORT=18008 # Matrix client API, plain HTTP (container port 8008)

FS_MSGTEST_JMAP_URL=http://$FS_MSGTEST_HOST:$FS_MSGTEST_JMAP_PORT
FS_MSGTEST_MATRIX_URL=http://$FS_MSGTEST_HOST:$FS_MSGTEST_MATRIX_PORT

# --- Mail. Log in with the full address. ---
FS_MSGTEST_MAIL_DOMAIN=msgtest.test
FS_MSGTEST_MAIL_HOSTNAME=mail.msgtest.test
FS_MSGTEST_MAIL_ADMIN_USER=admin
FS_MSGTEST_MAIL_ADMIN_PASSWORD=fs-msgtest-throwaway-admin
FS_MSGTEST_MAIL_USER1=alice@$FS_MSGTEST_MAIL_DOMAIN
FS_MSGTEST_MAIL_USER1_PASSWORD=fs-msgtest-alice-Throwaway-7421
FS_MSGTEST_MAIL_USER2=bob@$FS_MSGTEST_MAIL_DOMAIN
FS_MSGTEST_MAIL_USER2_PASSWORD=fs-msgtest-bob-Throwaway-7421

# --- Matrix. ---
FS_MSGTEST_MATRIX_SERVER_NAME=msgtest.test
FS_MSGTEST_MATRIX_REGISTRATION_TOKEN=fs-msgtest-throwaway-registration-token
FS_MSGTEST_MATRIX_USER1=alice
FS_MSGTEST_MATRIX_USER1_PASSWORD=fs-msgtest-alice-Throwaway-7421
FS_MSGTEST_MATRIX_USER2=bob
FS_MSGTEST_MATRIX_USER2_PASSWORD=fs-msgtest-bob-Throwaway-7421
