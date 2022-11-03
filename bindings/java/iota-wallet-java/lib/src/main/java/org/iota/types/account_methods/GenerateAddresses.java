// Copyright 2022 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

package org.iota.types.account_methods;

public class GenerateAddresses implements AccountMethod {

    private int amount;
    private AddressGenerationOptions options;

    public GenerateAddresses withAmount(int amount) {
        this.amount = amount;
        return this;
    }

    public GenerateAddresses withAddressGenerationOptions(AddressGenerationOptions options) {
        this.options = options;
        return this;
    }

    public static class AddressGenerationOptions {
        private boolean internal;
        private GenerateAddressMetadata metadata;

        public AddressGenerationOptions withInternal(boolean internal) {
            this.internal = internal;
            return this;
        }

        public AddressGenerationOptions withMetadata(GenerateAddressMetadata metadata) {
            this.metadata = metadata;
            return this;
        }

        public static class GenerateAddressMetadata {
            private boolean syncing;

            public GenerateAddressMetadata withSyncing(boolean syncing) {
                this.syncing = syncing;
                return this;
            }

        }
    }

}