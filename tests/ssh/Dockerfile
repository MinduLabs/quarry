# SSH server for the tunnel integration tests: user quarry / password quarry (test values only),
# TCP forwarding on, so it can forward to the `mysql` service.
FROM alpine:3.20
RUN apk add --no-cache openssh \
    && ssh-keygen -A \
    && adduser -D quarry \
    && echo "quarry:quarry" | chpasswd \
    && sed -i -e 's/^#\?AllowTcpForwarding.*/AllowTcpForwarding yes/' \
              -e 's/^#\?PasswordAuthentication.*/PasswordAuthentication yes/' /etc/ssh/sshd_config
EXPOSE 22
CMD ["/usr/sbin/sshd", "-D", "-e"]
