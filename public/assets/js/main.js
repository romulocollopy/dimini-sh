(function () {
  const ERROR_MESSAGE = "Sorry, something went wrong... \n\n";

  class Page {
    constructor(body) {
      this.form = body.querySelector("form");
      this.result_box = body.querySelector("div.result");
      this.result_text = this.result_box.querySelector("#shortened");
      this.error_text = this.result_box.querySelector("#error");
      this.message_box = body.querySelector("#message");
      this.init_form();
      this.host = `http://${document.location.host}`;
    }

    init_form() {
      this.form.addEventListener("submit", this.submit.bind(this));
    }

    submit(evt) {
      evt.preventDefault();
      this.post();
    }

    post() {
      const method = this.form.getAttribute("method"),
        path = this.form.getAttribute("action"),
        raw_url = this.form.querySelector("#original-url").value,
        short_code = this.form.querySelector("#short-code")?.value;

      var oReq = new XMLHttpRequest();
      oReq.addEventListener("load", this.onSuccess());
      oReq.addEventListener("error", this.onError.bind(this));

      oReq.open(method, path);
      oReq.setRequestHeader("Accept", "application/json");
      oReq.setRequestHeader(`Content-Type`, `application/json`);
      oReq.send(
        JSON.stringify({
          url: raw_url,
          short_code: short_code,
        }),
      );
    }

    onSuccess(a, b, c) {
      const _this = this;

      return function () {
        let response;
        if (this.status > 400) {
          return _this.onError().bind(this)();
        }

        try {
          const short_code = JSON.parse(this.responseText).short_code;
          response = `${_this.host}/${short_code}`;
        } catch {
          response = this.responseText;
        }
        _this.error_text.innerHTML = "";
        _this.result_text.innerHTML = response;
        _this.message_box.innerHTML = `access <br><a href="${response}/about" target="_blank();">${response}/about</a> for information about this url`;
      };
    }

    onError(resp) {
      const _this = this;
      return function () {
        resp = this.responseText || resp;
        _this.error_text.innerHTML = `${ERROR_MESSAGE} ${resp}`;
      };
    }
  }
  const body = document.querySelector("body");
  const page = new Page(body);
})();
