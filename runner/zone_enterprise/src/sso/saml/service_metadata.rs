use crate::sso::saml::canonical::escape_attribute;
use crate::sso::saml::service_provider::ServiceProvider;
use crate::sso::saml::uri;

pub const CONTENT_TYPE: &str = "application/samlmetadata+xml";

pub fn render(service: &ServiceProvider) -> String {
    let mut entity_id = String::new();
    escape_attribute(&service.entity_id, &mut entity_id);
    let mut acs_url = String::new();
    escape_attribute(&service.acs_url, &mut acs_url);
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<md:EntityDescriptor xmlns:md=\"{metadata}\" entityID=\"{entity_id}\">\n\
  <md:SPSSODescriptor AuthnRequestsSigned=\"false\" WantAssertionsSigned=\"true\" protocolSupportEnumeration=\"{protocol}\">\n\
    <md:NameIDFormat>{persistent}</md:NameIDFormat>\n\
    <md:NameIDFormat>{email}</md:NameIDFormat>\n\
    <md:AssertionConsumerService Binding=\"{post}\" Location=\"{acs_url}\" index=\"0\" isDefault=\"true\"/>\n\
  </md:SPSSODescriptor>\n\
</md:EntityDescriptor>\n",
        metadata = uri::METADATA,
        protocol = uri::PROTOCOL,
        persistent = uri::PERSISTENT,
        email = uri::EMAIL_ADDRESS,
        post = uri::HTTP_POST,
    )
}
