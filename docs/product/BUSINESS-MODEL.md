# Mô hình kinh doanh: open source + subscription

> Quyết định: ADR [0002](../architecture/adr/0002-agpl-single-repo-license-key.md).

## 1. Mô hình đã chốt: một repo public, code Pro mở, khoá bằng license key

Cách này giống TablePro: toàn bộ code nằm trong một repo AGPL-3.0, kể cả tính năng Pro.

```
Repo public MinduLabs/quarry (AGPL-3.0-or-later)
├── Tính năng Free:  driver, editor, grid, SSH, safe mode, export, AI (BYOK), MCP...
├── Tính năng Pro:   schema/data compare, ER diagram, chart, query insights, data rewind
│                    → gọi Entitlements::has(Feature::X) trước khi chạy
└── License client:  kích hoạt key → server trả payload ký Ed25519 → app tự xác thực offline

Server (dịch vụ trả phí)
├── License API:     kích hoạt, đếm số máy, gia hạn
└── Sync + Team:     sync mã hoá đầu-cuối, danh mục connection dùng chung, audit
                     → nguồn thu chính, không fork được
```

**Vì sao chọn cách này:**
- **Làm một mình, MVP chưa có tính năng Pro nào.** Dựng 2 repo hay thư mục có license riêng lúc này chỉ tốn công.
- **Người trả tiền là doanh nghiệp, và doanh nghiệp không dùng bản crack.**
- **Linux không có iCloud.** Sync bắt buộc phải chạy trên server của mình, nên chính server là phần thu tiền thật, không ai fork được.
- **Cộng đồng Linux ủng hộ open source thật.** Listing trên Flathub sạch sẽ.

**Khi nào thì đổi:** khi có bản fork gỡ license phát tán rộng, hoặc có đối thủ clone lại app. Lúc đó tính năng Pro *mới* chuyển vào thư mục `pro/` với license riêng (cách GitLab làm với `ee/`). Việc chuyển này hợp lệ nhờ có CLA.

## 2. Ranh giới Free / Pro

**Quy tắc:** cái gì một dev làm việc một mình cần hằng ngày thì **free mãi mãi**. Cái gì cho power user, team, hoặc tốn chi phí server thì vào **Pro**. Không bao giờ giới hạn kiểu "tối đa 3 connection".

| Free | Pro (cá nhân) | Team |
|---|---|---|
| Mọi DB, mọi driver | Sync qua nhiều máy (E2E) | Mọi thứ trong Pro |
| SQL editor, grid, sửa dữ liệu | Schema compare + migration script | Danh mục connection dùng chung (phân quyền) |
| SSH, SSL, safe mode | Data compare/sync | Thư viện query chung |
| Export/import cơ bản | ER diagram, chart | Audit log |
| AI dùng key riêng, MCP | Query insights, Data rewind | SSO/SAML |

## 3. Pháp lý

1. **License AGPL-3.0-or-later.** Engine chạy headless được, nên ai đó có thể bọc nó thành dịch vụ web. AGPL buộc họ phải mở code nếu làm vậy. Doanh nghiệp chỉ cài app về dùng thì không bị ảnh hưởng gì.
2. **CLA từ PR đầu tiên** (CLA Assistant). CLA giữ quyền relicense: sau này có thể tách `pro/` ra, hoặc bán license thương mại cho công ty không chấp nhận AGPL.
3. **`cargo-deny`** chặn dependency GPL/AGPL của bên thứ ba, để giữ đường lui ở trên.
4. **Trademark** cho tên và logo "Quarry". Code ai fork cũng được, nhưng tên thì không.

## 4. License key

- Payload `{email, plan, seats, issued_at, expires_at, features}` + chữ ký Ed25519. Public key nhúng sẵn trong app.
- Có kết nối mạng thì kích hoạt và kiểm tra lại định kỳ. Không có mạng thì key vẫn dùng được; **không bao giờ khoá dữ liệu người dùng**.
- Grace period 14 ngày. Có **perpetual fallback**: trả đủ 12 tháng thì được giữ vĩnh viễn phiên bản lúc đó.

## 5. Thanh toán

Stripe không mở tài khoản trực tiếp cho doanh nghiệp Việt Nam, nên dùng Merchant of Record (họ đứng ra thu tiền và lo thuế): **Polar.sh** hoặc **Lemon Squeezy**, cả hai đều có sẵn license key API. Paddle là phương án dự phòng.

## 6. Giá (giả thuyết, kiểm chứng ở v1.0)

| Gói | Giá |
|---|---|
| Free | $0 |
| Pro | $6/tháng hoặc $59/năm, tối đa 3 máy |
| Team | $12/user/tháng, tối thiểu 3 seat |
| Sinh viên, maintainer OSS | Miễn phí |

## 7. Lộ trình doanh thu

| Version | Doanh thu |
|---|---|
| v0.1–0.3 | GitHub Sponsors / Open Collective, sponsor từ các nhà cung cấp DB |
| v1.0 | Ra mắt Pro + Sync |
| v1.x | Team |
