#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.10"
# dependencies = [
#     "google-cloud-pubsub",
#     "google",
#     "google-cloud-storage",
# ]
# ///
import argparse
import base64
import enum
import json
import logging
import os
import shutil
import sys
import tempfile
import time
import uuid
from dataclasses import asdict, dataclass

from google.api_core.exceptions import NotFound  # type: ignore
from google.cloud import pubsub_v1  # type: ignore
from google.oauth2.service_account import Credentials  # type: ignore
from lc_py.lc_config import JSONConfig  # nopep8
from lc_py.lc_gsifile import LCGSIFile  # nopep8
from lc_py.lc_py_utils import LcUtil  # nopep8

DEF_SIGN_TIMEOUT = (20 * 60)
ENV_BASE64_KEY_NAMES = ("CODE_SIGNING_KEY_B64", "CODE_SIGNING_KEY")


class SignFileType(enum.Enum):
    SENSOR_ARCHIVE = 1
    PACKAGE_ARCHIVE = 2
    HLK_ARCHIVE = 3


@dataclass
class SigningRequest:
    file_type: int
    unsigned_uri: str
    signed_uri: str


class InvalidArgumentError(Exception):
    pass


def get_env_base64_key() -> str | None:
    """Gets the base64 key from environment variables.

    Parameters:
        None.

    Return:
        The base64 key if present, otherwise None.
    """
    # Prefer explicitly named vars to reduce accidental leakage via env.
    for env_name in ENV_BASE64_KEY_NAMES:
        env_value = os.environ.get(env_name)
        if env_value:
            return env_value
    return None




class SigningPublisher:

    def __init__(self, project: str, topic: str, bucket_name: str, key: str) -> None:
        self.project = project
        self.topic = topic
        self.key = key
        self.bucket_name = bucket_name
        self.creds = Credentials.from_service_account_file(key)  # type: ignore

    def __enter__(self) -> 'SigningPublisher':
        # Code to run when entering the context
        logging.info(f"authenticating using {self.key}")
        self.pub = pubsub_v1.PublisherClient(  # type: ignore
            credentials=self.creds)  # type: ignore
        return self

    def __exit__(self, exc_type, exc_value, traceback):  # type: ignore
        pass

    def sign_archive(self, sign_type: SignFileType, input: str, output: str, timeout: float) -> None:

        logging.info(f"Input file: {input}")
        logging.info(f"Output file: {output}")

        # upload file to a bucket
        gsi = LCGSIFile(self.key)

        uuid_str = str(uuid.uuid4())

        file_name = f"lc_sensor_{uuid_str}.zip"

        try:

            uri = gsi.upload(input, self.bucket_name, file_name)

            signed_uri = uri + ".signed"

            req = SigningRequest(sign_type.value, uri, signed_uri)

            message_json = json.dumps(asdict(req))
            message = base64.b64encode(message_json.encode("utf-8"))

            topic_path = self.pub.topic_path(self.project,  # type: ignore
                                             self.topic)
            future = self.pub.publish(topic_path, message)  # type: ignore

            msg_id = future.result()  # type: ignore

            logging.info(f"message id: {msg_id}")

            # wait for the signed file to appear
            cur_ts = time.time()
            end_ts = cur_ts + timeout

            while cur_ts < end_ts:

                try:
                    gsi.download_uri(signed_uri, output)
                    gsi.delete_uri(signed_uri)
                    break
                except NotFound:
                    pass

                time.sleep(5)
                cur_ts = time.time()

                rem_secs = end_ts - cur_ts

                logging.info(f"timeout={rem_secs:.2f}")

            if cur_ts > end_ts:
                raise TimeoutError(f"Unable to sign {input} in time")

        finally:
            gsi.delete(self.bucket_name, file_name)

    def sign(self, sign_type: SignFileType, input: str, output: str, timeout: float) -> None:

        if input.endswith(".zip"):
            self.sign_archive(sign_type, input, output, timeout)
            return

        with tempfile.TemporaryDirectory(prefix="sign_file_") as td:

            input_fn = os.path.basename(input)

            tmp_bin = os.path.join(td, "bin")
            os.mkdir(tmp_bin)

            unsigned_input = os.path.join(tmp_bin, input_fn)
            shutil.copy2(input, unsigned_input)

            tmp_zip = os.path.join(td, "bin.zip")

            LcUtil.zip(tmp_bin, tmp_zip, include_root=False)

            self.sign_archive(sign_type, tmp_zip, tmp_zip, timeout)

            signed_bin = os.path.join(td, "signed_bin")
            os.mkdir(signed_bin)

            LcUtil.unzip(tmp_zip, signed_bin)

            signed_output = os.path.join(signed_bin, input_fn)

            shutil.copy2(signed_output, output)

    def sign_macos(self, input: str, entitlements: str, output: str, timeout: float) -> None:

        with tempfile.TemporaryDirectory(prefix="macos_sign_") as td:

            input_fn = os.path.basename(input)
            bin_file = os.path.join(td, input_fn)
            shutil.copy2(input, bin_file)

            entitlements_dir = os.path.join(td, "entitlements")
            os.mkdir(entitlements_dir)

            e_file = os.path.join(entitlements_dir, f"{input_fn}.plist")
            shutil.copy2(entitlements, e_file)

            e_zip = os.path.join(td, "entitlements.zip")

            LcUtil.zip(entitlements_dir, e_zip)

            shutil.rmtree(entitlements_dir)

            with tempfile.TemporaryDirectory(prefix="unsigned") as unsigned:

                sign_package = os.path.join(unsigned, "package.zip")

                LcUtil.zip(td, sign_package, include_root=False)

                # this may take a while
                self.sign(SignFileType.SENSOR_ARCHIVE,
                          sign_package,
                          sign_package,
                          timeout)

                # unzip and extract the signed file
                LcUtil.unzip(sign_package, td)

            # return the signed / notarized file
            shutil.copy2(bin_file, output)


def main() -> int:

    status = 1

    parser = argparse.ArgumentParser()

    script_root = os.path.abspath(os.path.dirname(sys.argv[0]))
    config_file = os.path.join(script_root, "config.json")

    config = JSONConfig(config_file)

    def_topic = config.get("/pub/topic")
    def_bucket = config.get("/pub/bucket")
    def_project = config.get("/project")

    parser.add_argument("-k",
                        "--key",
                        type=str,
                        help="DEPRECATED: Use env var instead.")

    parser.add_argument("--base64-key",
                        type=str,
                        help="DEPRECATED: Use env var instead.")


    parser.add_argument("-v",
                        "--verbose",
                        action="store_true",
                        help="log to stdout")

    parser.add_argument("-i",
                        "--input",
                        type=str,
                        required=True,
                        help="/path/to/lc_sensor.zip")

    parser.add_argument("-o",
                        "--output",
                        type=str,
                        help="/path/to/lc_sensor_signed.zip")

    parser.add_argument("-p",
                        "--project",
                        type=str,
                        default=def_project,
                        help=f"Google sub project. Default: {def_project}")

    parser.add_argument("-t",
                        "--topic",
                        type=str,
                        default=def_topic,
                        help=f"Topic. Default: {def_topic}")

    parser.add_argument("-b",
                        "--bucket",
                        type=str,
                        default=def_bucket,
                        help=f"Bucket name. Default: {def_bucket}")

    parser.add_argument("--timeout",
                        type=float,
                        default=DEF_SIGN_TIMEOUT,
                        help=f"Signing timeout. Default: {DEF_SIGN_TIMEOUT}")

    parser.add_argument("--sign-type",
                        type=str,
                        default="sensor",
                        choices=["sensor", "package", "hlk"],
                        help="Type of signing")

    parser.add_argument("-e",
                        "--entitlements-file",
                        type=str,
                        help="/path/to/entitlements.plist")

    args = parser.parse_args()

    try:
        LcUtil.init_logging("code_signing.log", args.verbose)

        args.input = os.path.abspath(args.input)

        if args.output is None:
            args.output = args.input

        print("Signing Publisher:")
        LcUtil.printkv("Input File", args.input)
        LcUtil.printkv("Input File Size", LcUtil.file_size_fmt(args.input))
        LcUtil.printkv("Input File Hash", LcUtil.md5_file(args.input))
        LcUtil.printkv("Signing Type", args.sign_type)

        env_base64_key = get_env_base64_key()

        if args.key is not None:
            # Warn to discourage CLI secrets while still failing closed.
            logging.warning("--key was provided via CLI; env vars are preferred")
            raise InvalidArgumentError(
                "--key is disabled; use env var instead")
        if args.base64_key is not None:
            # Warn to discourage CLI secrets while still failing closed.
            logging.warning("--base64-key was provided via CLI; env vars are preferred")
            raise InvalidArgumentError(
                "--base64-key is disabled; use env var instead")

        if env_base64_key is None:
            raise InvalidArgumentError(
                "env var is missing")
        if env_base64_key.strip() == "":
            raise InvalidArgumentError(
                "env var is empty")

        LcUtil.printkv("Project", args.project)
        LcUtil.printkv("Topic", args.topic)
        LcUtil.printkv("Bucket Name", args.bucket)
        LcUtil.printkv("Signing timeout", args.timeout)

        if args.entitlements_file is not None:
            LcUtil.printkv("Entitlements", args.entitlements_file)

        with tempfile.TemporaryDirectory(prefix="pub_client_") as td:

            if env_base64_key is not None:
                # Write the env-provided key to a temp file for GCP auth.
                key_file = os.path.join(td, "key.json")
                LcUtil.b64_to_file(env_base64_key, key_file)
            else:
                raise InvalidArgumentError("env var is missing")

            if args.sign_type == "sensor":
                sign_type = SignFileType.SENSOR_ARCHIVE
            elif args.sign_type == "package":
                sign_type = SignFileType.PACKAGE_ARCHIVE
            elif args.sign_type == "hlk":
                sign_type = SignFileType.HLK_ARCHIVE
            else:
                raise NotImplementedError()

            with SigningPublisher(args.project,
                                  args.topic,
                                  args.bucket,
                                  key_file) as pub:

                if args.entitlements_file is None:
                    pub.sign(sign_type, args.input, args.output, args.timeout)
                else:
                    pub.sign_macos(args.input,
                                   args.entitlements_file,
                                   args.output,
                                   args.timeout)

        LcUtil.printkv("Output File", args.output)
        LcUtil.printkv("Output File Size", LcUtil.file_size_fmt(args.output))
        LcUtil.printkv("Output File Hash", LcUtil.md5_file(args.output))

        status = 0
    except InvalidArgumentError as e:
        print(e)
    except KeyboardInterrupt:
        pass

    return status


if __name__ == '__main__':

    status = main()

    if 0 != status:
        sys.exit(status)
